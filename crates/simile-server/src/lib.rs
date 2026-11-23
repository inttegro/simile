#![forbid(unsafe_code)]

mod delivery;
mod process;
mod proxy;

pub use process::CommandResult;
pub use proxy::{ProxyOptions, ProxyServer};

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::{Body, Bytes, HttpBody, to_bytes};
use axum::extract::{ConnectInfo, State};
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN, ALLOW,
    CONTENT_TYPE, COOKIE, ORIGIN, USER_AGENT, VARY,
};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use futures_util::stream;
use serde::{Deserialize, Serialize};
use serde_json::json;
use simile_artifact::Artifact;
use simile_runtime::{
    ExecutionOptions, IncomingParameters, OperationCall, OperationExecution, ReplayIdentity,
    Runtime, TransportPlan, validate_parameters,
};
use simile_service::Service;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

const DEFAULT_BODY_LIMIT: usize = 8 * 1024 * 1024;
const INSPECTION_EXCHANGE_LIMIT: usize = 200;
const INSPECTION_BODY_LIMIT: usize = 256 * 1024;
const INSPECTION_PREFIX: &str = "/.simile/";

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("could not bind service to {address}: {source}")]
    Bind {
        address: SocketAddr,
        #[source]
        source: std::io::Error,
    },

    #[error("could not inspect the bound service address: {0}")]
    LocalAddress(#[source] std::io::Error),

    #[error("service listener failed: {0}")]
    Serve(#[source] std::io::Error),

    #[error("compiled service has no scenario named `{0}`")]
    UnknownScenario(String),

    #[error("compiled service has no fault profile named `{0}`")]
    UnknownFaultProfile(String),

    #[error("could not initialize fault profile `{profile}`: {message}")]
    FaultProfileState { profile: String, message: String },

    #[error("could not initialize service state: {0}")]
    Service(#[from] simile_service::Error),

    #[error("a test command is required")]
    CommandMissing,

    #[error("could not start test command `{command}`")]
    CommandSpawn {
        command: String,
        #[source]
        source: std::io::Error,
    },

    #[error("could not wait for the test command")]
    CommandWait(#[source] std::io::Error),

    #[error("could not listen for test interruption")]
    CommandSignal(#[source] std::io::Error),

    #[error("the service stopped before the test command completed")]
    CommandServiceStopped,

    #[error("the supervised service task failed")]
    CommandServiceTask(#[source] tokio::task::JoinError),
}

#[derive(Clone, Debug, Default)]
pub struct ServerOptions {
    pub scenario: Option<String>,
    pub fault_profile: Option<String>,
    pub reporting_sender: Option<mpsc::Sender<RuntimeReportCommand>>,
}

/// Sanitized evidence for one internal runtime failure that may become an automatic report.
#[derive(Clone, Debug)]
pub struct RuntimeIncident {
    pub profile: Option<String>,
    pub operation: String,
    pub seed: u64,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    pub parameters: IncomingParameters,
    pub scenario: Option<String>,
    pub fault_profile: Option<String>,
    pub code: String,
    pub message: String,
    pub issues: Vec<serde_json::Value>,
}

#[derive(Clone, Debug)]
pub struct RuntimeExchangeReport {
    pub exchange_id: String,
    pub profile: Option<String>,
    pub operation: String,
    pub response_seed: u64,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    pub parameters: IncomingParameters,
    pub outcome: Option<String>,
    pub scenario: Option<String>,
    pub fault_profile: Option<String>,
    pub expected: RuntimeExpectedResponse,
}

#[derive(Clone, Debug)]
pub struct RuntimeExpectedResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeReportSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub created_at: String,
    pub service: String,
    pub operation: String,
    pub artifact_digest: String,
    pub verified: bool,
    pub checked_at: String,
    pub fingerprint: String,
    pub submitted: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub upgrade_recommendations: Vec<RuntimeUpgradeRecommendation>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeUpgradeRecommendation {
    pub component: String,
    pub current: String,
    pub available: String,
    pub command: String,
}

#[derive(Clone, Debug)]
pub struct RuntimeReportError {
    pub code: String,
    pub message: String,
}

#[derive(Debug)]
pub enum RuntimeReportCommand {
    Automatic(RuntimeIncident),
    Prepare {
        exchange: RuntimeExchangeReport,
        title: String,
        description: Option<String>,
        reply: oneshot::Sender<std::result::Result<RuntimeReportSummary, RuntimeReportError>>,
    },
    Submit {
        report: String,
        contact: Option<String>,
        current_version: bool,
        reply: oneshot::Sender<std::result::Result<RuntimeReportSummary, RuntimeReportError>>,
    },
}

pub struct Server {
    listener: TcpListener,
    router: Router,
    address: SocketAddr,
    fault_seed: Option<u64>,
    service: Arc<Service>,
}

struct ServerState {
    service: Arc<Service>,
    routes: RouteTable,
    body_limit: usize,
    inspector: Option<Arc<Inspector>>,
    scenario: Option<String>,
    fault_profile: Option<String>,
    reporting_sender: Option<mpsc::Sender<RuntimeReportCommand>>,
}

#[derive(Default)]
struct Inspector {
    next_exchange: AtomicU64,
    exchanges: Mutex<VecDeque<RecordedExchange>>,
}

#[derive(Clone, Debug)]
struct RecordedExchange {
    exchange: InspectionExchange,
    report_capture: Option<RuntimeExchangeReport>,
    report: Option<RuntimeReportSummary>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InspectionExchange {
    id: String,
    operation: String,
    method: String,
    path: String,
    started_at_ms: u64,
    duration_micros: u64,
    status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
    request: InspectionMessage,
    response: InspectionMessage,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InspectionMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<serde_json::Value>,
    truncated: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrepareRuntimeReportBody {
    title: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmitRuntimeReportBody {
    contact: Option<String>,
    #[serde(default)]
    current_version: bool,
}

#[derive(Debug, Default, Deserialize)]
struct ConfirmDestructiveActionBody {
    #[serde(default)]
    confirm: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RouteTable {
    routes: Vec<Route>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Route {
    method: String,
    path: PathTemplate,
    operation: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PathTemplate {
    segments: Vec<PathSegment>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PathSegment {
    Literal(String),
    Parameter(String),
}

enum Match<'a> {
    Operation {
        operation: &'a str,
        path: BTreeMap<String, Vec<String>>,
    },
    MethodNotAllowed(BTreeSet<&'a str>),
    NotFound,
}

struct AccessLine<'a> {
    method: &'a Method,
    path: &'a str,
    status: StatusCode,
    origin_ip: Option<IpAddr>,
    user_agent: Option<&'a str>,
    request_bytes: Option<u64>,
    request_content_type: Option<&'a str>,
    response_bytes: Option<u64>,
    response_content_type: Option<&'a str>,
    elapsed: Duration,
}

impl Server {
    pub async fn bind(address: SocketAddr, artifact: Artifact) -> Result<Self> {
        Self::bind_with_options(address, artifact, ServerOptions::default()).await
    }

    pub async fn bind_with_scenario(
        address: SocketAddr,
        artifact: Artifact,
        scenario: Option<String>,
    ) -> Result<Self> {
        Self::bind_with_options(
            address,
            artifact,
            ServerOptions {
                scenario,
                fault_profile: None,
                reporting_sender: None,
            },
        )
        .await
    }

    pub async fn bind_with_options(
        address: SocketAddr,
        artifact: Artifact,
        options: ServerOptions,
    ) -> Result<Self> {
        let service = Service::memory(artifact)?;
        Self::bind_service_with_options(address, service, options).await
    }

    /// Bind a service and persist its complete Stack at `state_path`.
    pub async fn bind_durable_with_options(
        address: SocketAddr,
        artifact: Artifact,
        state_path: impl AsRef<Path>,
        options: ServerOptions,
    ) -> Result<Self> {
        let service = Service::durable(artifact, state_path)?;
        Self::bind_service_with_options(address, service, options).await
    }

    /// Bind a service whose state is owned by the supplied supervisor.
    pub async fn bind_service_with_options(
        address: SocketAddr,
        service: Service,
        options: ServerOptions,
    ) -> Result<Self> {
        let artifact = service.artifact();
        if let Some(name) = options.scenario.as_deref() {
            if !artifact.has_scenario(name) {
                return Err(Error::UnknownScenario(name.to_owned()));
            }
        }
        if let Some(name) = options.fault_profile.as_deref() {
            if !artifact.has_fault_profile(name) {
                return Err(Error::UnknownFaultProfile(name.to_owned()));
            }
        }
        let listener = TcpListener::bind(address)
            .await
            .map_err(|source| Error::Bind { address, source })?;
        let address = listener.local_addr().map_err(Error::LocalAddress)?;
        let routes = RouteTable::new(artifact);
        let fault_seed = options
            .fault_profile
            .as_deref()
            .map(|profile| {
                service
                    .runtime()
                    .fault_profile_seed(profile)
                    .map_err(|error| Error::FaultProfileState {
                        profile: profile.to_owned(),
                        message: error.to_string(),
                    })
            })
            .transpose()?;
        let service = Arc::new(service);
        let inspector = address
            .ip()
            .is_loopback()
            .then(|| Arc::new(Inspector::default()));
        let state = Arc::new(ServerState {
            service: Arc::clone(&service),
            routes,
            body_limit: DEFAULT_BODY_LIMIT,
            inspector,
            scenario: options.scenario,
            fault_profile: options.fault_profile,
            reporting_sender: options.reporting_sender,
        });
        let router = Router::new()
            .fallback(handle)
            .layer(middleware::from_fn(access_log))
            .with_state(state);
        Ok(Self {
            listener,
            router,
            address,
            fault_seed,
            service,
        })
    }

    #[must_use]
    pub const fn local_addr(&self) -> SocketAddr {
        self.address
    }

    #[must_use]
    pub const fn fault_seed(&self) -> Option<u64> {
        self.fault_seed
    }

    pub async fn run(self) -> Result<()> {
        self.run_until(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::warn!(%error, "could not install Ctrl-C handler");
            }
        })
        .await
    }

    pub async fn run_until<F>(self, shutdown: F) -> Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let dispatcher = tokio::spawn(delivery::run(Arc::clone(&self.service)));
        let result = axum::serve(
            self.listener,
            self.router
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(Error::Serve);
        dispatcher.abort();
        result
    }
}

impl fmt::Display for AccessLine<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {} {} origin=",
            self.method,
            self.path,
            self.status.as_u16()
        )?;
        write_ip(formatter, self.origin_ip)?;
        formatter.write_str(" user-agent=")?;
        write_label(formatter, self.user_agent)?;
        formatter.write_str(" request=")?;
        write_size(formatter, self.request_bytes)?;
        formatter.write_str(" request-type=")?;
        write_label(formatter, self.request_content_type)?;
        formatter.write_str(" response=")?;
        write_size(formatter, self.response_bytes)?;
        formatter.write_str(" response-type=")?;
        write_label(formatter, self.response_content_type)?;
        write!(formatter, " elapsed={}us", self.elapsed.as_micros())
    }
}

fn write_ip(formatter: &mut fmt::Formatter<'_>, address: Option<IpAddr>) -> fmt::Result {
    match address {
        Some(address) => write!(formatter, "{address}"),
        None => formatter.write_str("-"),
    }
}

fn write_size(formatter: &mut fmt::Formatter<'_>, bytes: Option<u64>) -> fmt::Result {
    match bytes {
        Some(bytes) => write!(formatter, "{bytes}B"),
        None => formatter.write_str("?"),
    }
}

fn write_label(formatter: &mut fmt::Formatter<'_>, value: Option<&str>) -> fmt::Result {
    match value {
        Some(value) => write!(formatter, "{value:?}"),
        None => formatter.write_str("-"),
    }
}

impl RouteTable {
    fn new(artifact: &Artifact) -> Self {
        let mut routes = artifact
            .operations()
            .map(|operation| Route {
                method: operation.method().to_owned(),
                path: PathTemplate::parse(operation.path()),
                operation: operation.key().to_owned(),
            })
            .collect::<Vec<_>>();
        routes.sort_by(|left, right| {
            right
                .path
                .literal_count()
                .cmp(&left.path.literal_count())
                .then_with(|| left.operation.cmp(&right.operation))
        });
        Self { routes }
    }

    fn find(&self, method: &Method, path: &str) -> Match<'_> {
        let mut allowed = BTreeSet::new();
        for route in &self.routes {
            let Some(parameters) = route.path.match_and_capture(path) else {
                continue;
            };
            if route.method == method.as_str() {
                return Match::Operation {
                    operation: &route.operation,
                    path: parameters,
                };
            }
            allowed.insert(route.method.as_str());
        }
        if allowed.is_empty() {
            Match::NotFound
        } else {
            Match::MethodNotAllowed(allowed)
        }
    }
}

impl PathTemplate {
    fn parse(path: &str) -> Self {
        let segments = split_path(path)
            .map(|segment| {
                if segment.starts_with('{') && segment.ends_with('}') {
                    PathSegment::Parameter(
                        segment
                            .trim_start_matches('{')
                            .trim_end_matches('}')
                            .to_owned(),
                    )
                } else {
                    PathSegment::Literal(segment.to_owned())
                }
            })
            .collect();
        Self { segments }
    }

    fn match_and_capture(&self, path: &str) -> Option<BTreeMap<String, Vec<String>>> {
        let actual = split_path(path).collect::<Vec<_>>();
        if self.segments.len() != actual.len() {
            return None;
        }
        let mut captured = BTreeMap::new();
        for (expected, actual) in self.segments.iter().zip(actual) {
            match expected {
                PathSegment::Literal(expected) if expected == actual => {}
                PathSegment::Literal(_) => return None,
                PathSegment::Parameter(name) if !actual.is_empty() => {
                    let decoded = percent_encoding::percent_decode_str(actual)
                        .decode_utf8()
                        .ok()?
                        .into_owned();
                    captured.insert(name.clone(), vec![decoded]);
                }
                PathSegment::Parameter(_) => return None,
            }
        }
        Some(captured)
    }

    fn literal_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| matches!(segment, PathSegment::Literal(_)))
            .count()
    }
}

fn split_path(path: &str) -> impl Iterator<Item = &str> {
    path.trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
}

async fn access_log(request: Request<Body>, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let origin_ip = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|connection| connection.0.ip());
    let user_agent = text_header(request.headers(), USER_AGENT.as_str());
    let request_bytes = request.body().size_hint().exact();
    let request_content_type = text_header(request.headers(), CONTENT_TYPE.as_str());
    let started = Instant::now();
    let response = next.run(request).await;
    let response_content_type = text_header(response.headers(), CONTENT_TYPE.as_str());
    let line = AccessLine {
        method: &method,
        path: &path,
        status: response.status(),
        origin_ip,
        user_agent: user_agent.as_deref(),
        request_bytes,
        request_content_type: request_content_type.as_deref(),
        response_bytes: response.body().size_hint().exact(),
        response_content_type: response_content_type.as_deref(),
        elapsed: started.elapsed(),
    };
    let origin = line.origin_ip.map(|value| value.to_string());
    tracing::info!(
        target: "simile::http",
        method = %line.method,
        path = line.path,
        status = line.status.as_u16(),
        origin = origin.as_deref().unwrap_or("-"),
        user_agent = line.user_agent.unwrap_or("-"),
        request_bytes = line.request_bytes,
        request_type = line.request_content_type.unwrap_or("-"),
        response_bytes = line.response_bytes,
        response_type = line.response_content_type.unwrap_or("-"),
        elapsed_us = line.elapsed.as_micros(),
        "request completed"
    );
    response
}

fn text_header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

async fn handle(State(state): State<Arc<ServerState>>, request: Request<Body>) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    if uri.path().starts_with(INSPECTION_PREFIX) {
        return inspection_response(&state, request).await;
    }
    let started = Instant::now();
    let started_at_ms = unix_time_millis();
    let (operation, path_parameters) = match state.routes.find(&method, uri.path()) {
        Match::Operation { operation, path } => (operation.to_owned(), path),
        Match::MethodNotAllowed(methods) => return method_not_allowed(methods),
        Match::NotFound => {
            return error_response(StatusCode::NOT_FOUND, "not_found", "route not found");
        }
    };
    let incoming = incoming_parameters(&request, path_parameters);
    let authentication = match state.service.authenticate(&operation, &incoming) {
        Ok(authentication) => authentication,
        Err(simile_service::Error::Runtime(error)) => {
            return operation_error(
                state.service.runtime(),
                &operation,
                error,
                method == Method::HEAD,
            );
        }
        Err(error) => {
            tracing::error!(target: "simile::auth", %operation, %error, "could not read service profiles");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "profile_state_unavailable",
                "service profile state is unavailable",
            );
        }
    };
    let parameters = match validate_parameters(state.service.artifact(), &operation, &incoming) {
        Ok(parameters) => parameters,
        Err(error) => {
            return operation_error(
                state.service.runtime(),
                &operation,
                error,
                method == Method::HEAD,
            );
        }
    };
    let content_type = match header(request.headers(), CONTENT_TYPE.as_str()) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let idempotency_key = match header(request.headers(), "idempotency-key") {
        Ok(value) => value,
        Err(response) => return response,
    };
    let resource_identity = uri.to_string();
    let body = match to_bytes(request.into_body(), state.body_limit).await {
        Ok(body) => body,
        Err(_) => {
            return error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request_too_large",
                "request body exceeds the configured limit",
            );
        }
    };
    let replay = idempotency_key
        .as_deref()
        .map(ReplayIdentity::IdempotencyKey)
        .or_else(|| {
            (method == Method::GET).then_some(ReplayIdentity::Resource(&resource_identity))
        });
    let response_seed = rand::random::<u64>();
    let profile = authentication.profile().map(str::to_owned);
    let exchange_id = state
        .inspector
        .as_ref()
        .map(|inspector| inspector.next_id(started_at_ms));
    let outcome = state.service.execute_for_profile_with_options(
        OperationCall {
            operation: &operation,
            content_type: content_type.as_deref(),
            body: &body,
        },
        ExecutionOptions {
            parameters: Some(&parameters),
            replay,
            scenario: state.scenario.as_deref(),
            fault_profile: state.fault_profile.as_deref(),
            response_seed: Some(response_seed),
            ..ExecutionOptions::default()
        },
        profile.as_deref(),
    );
    match outcome {
        Ok(execution) => {
            let status = execution.response().status();
            let selected_outcome = execution.selected_outcome().map(str::to_owned);
            let response_content_type = execution.response().media_type().map(str::to_owned);
            let mut expected_body = execution.response().value().cloned();
            if let Some(body) = expected_body.as_mut() {
                redact_json(body);
            }
            let response_body = inspection_response_body(
                response_content_type.as_deref(),
                execution.response().bytes(),
            );
            let request_message = inspection_request_body(content_type.as_deref(), &body);
            let response = operation_execution_response(execution, method == Method::HEAD).await;
            if let (Some(inspector), Some(exchange_id)) = (state.inspector.as_ref(), exchange_id) {
                let report_capture = sanitize_body(content_type.as_deref(), &body).map(|body| {
                    RuntimeExchangeReport {
                        exchange_id: exchange_id.clone(),
                        profile: profile.clone(),
                        operation: operation.clone(),
                        response_seed,
                        content_type: content_type.clone(),
                        body,
                        parameters: sanitize_parameters(incoming.clone()),
                        outcome: selected_outcome.clone(),
                        scenario: state.scenario.clone(),
                        fault_profile: state.fault_profile.clone(),
                        expected: RuntimeExpectedResponse {
                            status,
                            content_type: response_content_type.clone(),
                            body: expected_body,
                        },
                    }
                });
                inspector.record(RecordedExchange {
                    exchange: InspectionExchange {
                        id: exchange_id,
                        operation,
                        method: method.to_string(),
                        path: uri.path().to_owned(),
                        started_at_ms,
                        duration_micros: started
                            .elapsed()
                            .as_micros()
                            .try_into()
                            .unwrap_or(u64::MAX),
                        status,
                        outcome: selected_outcome,
                        profile,
                        request: request_message,
                        response: response_body,
                    },
                    report_capture,
                    report: None,
                });
            }
            response
        }
        Err(simile_service::Error::Runtime(error)) => {
            emit_incident(
                &state,
                profile,
                &operation,
                response_seed,
                content_type,
                &body,
                incoming,
                &error,
            );
            operation_error(
                state.service.runtime(),
                &operation,
                error,
                method == Method::HEAD,
            )
        }
        Err(error) => {
            tracing::error!(target: "simile::effect", %operation, %error, "could not apply operation effects");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "effect_failed",
                "the operation completed but its declared effects could not be committed",
            )
        }
    }
}

impl Inspector {
    fn next_id(&self, started_at_ms: u64) -> String {
        let sequence = self.next_exchange.fetch_add(1, Ordering::Relaxed) + 1;
        format!("ex_{started_at_ms}_{sequence}")
    }

    fn record(&self, exchange: RecordedExchange) {
        let Ok(mut exchanges) = self.exchanges.lock() else {
            tracing::warn!(target: "simile::inspection", "exchange recorder is unavailable");
            return;
        };
        exchanges.push_front(exchange);
        exchanges.truncate(INSPECTION_EXCHANGE_LIMIT);
    }

    fn snapshot(&self) -> Option<Vec<InspectionExchange>> {
        self.exchanges.lock().ok().map(|exchanges| {
            exchanges
                .iter()
                .map(|recorded| recorded.exchange.clone())
                .collect()
        })
    }

    fn clear(&self) -> Option<usize> {
        self.exchanges.lock().ok().map(|mut exchanges| {
            let removed = exchanges.len();
            exchanges.clear();
            removed
        })
    }

    fn report_capture(&self, exchange_id: &str) -> Option<RuntimeExchangeReport> {
        self.exchanges.lock().ok().and_then(|exchanges| {
            exchanges
                .iter()
                .find(|recorded| recorded.exchange.id == exchange_id)
                .and_then(|recorded| recorded.report_capture.clone())
        })
    }

    fn report(&self, exchange_id: &str) -> Option<RuntimeReportSummary> {
        self.exchanges.lock().ok().and_then(|exchanges| {
            exchanges
                .iter()
                .find(|recorded| recorded.exchange.id == exchange_id)
                .and_then(|recorded| recorded.report.clone())
        })
    }

    fn set_report(&self, exchange_id: &str, report: RuntimeReportSummary) {
        let Ok(mut exchanges) = self.exchanges.lock() else {
            tracing::warn!(target: "simile::inspection", "exchange recorder is unavailable");
            return;
        };
        if let Some(recorded) = exchanges
            .iter_mut()
            .find(|recorded| recorded.exchange.id == exchange_id)
        {
            recorded.report = Some(report);
        }
    }

    fn update_report(&self, report: RuntimeReportSummary) {
        let Ok(mut exchanges) = self.exchanges.lock() else {
            tracing::warn!(target: "simile::inspection", "exchange recorder is unavailable");
            return;
        };
        for recorded in &mut *exchanges {
            if recorded
                .report
                .as_ref()
                .is_some_and(|prepared| prepared.id == report.id)
            {
                recorded.report = Some(report.clone());
            }
        }
    }
}

async fn inspection_response(state: &ServerState, request: Request<Body>) -> Response {
    let Some(inspector) = state.inspector.as_ref() else {
        return error_response(StatusCode::NOT_FOUND, "not_found", "route not found");
    };
    let origin = text_header(request.headers(), ORIGIN.as_str());
    if origin
        .as_deref()
        .is_some_and(|origin| !is_loopback_origin(origin))
    {
        return error_response(
            StatusCode::FORBIDDEN,
            "inspection_origin_forbidden",
            "runtime inspection accepts browser requests only from a loopback origin",
        );
    }
    if request.method() == Method::OPTIONS {
        return with_inspection_cors(StatusCode::NO_CONTENT.into_response(), origin.as_deref());
    }
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let response = match path.as_str() {
        "/.simile/inspection" if method == Method::GET => {
            match state.service.inspect_state(false) {
                Ok(inspection) => {
                    let summary = state.service.artifact().summary();
                    let exchange_count =
                        inspector.snapshot().map_or(0, |exchanges| exchanges.len());
                    json_response(
                        StatusCode::OK,
                        json!({
                            "status": "ready",
                            "service": summary.service,
                            "serviceVersion": summary.service_version,
                            "operations": summary.operations,
                            "resources": inspection.resources.len(),
                            "events": inspection.events.len(),
                            "exchanges": exchange_count,
                        }),
                    )
                }
                Err(error) => inspection_state_error(error),
            }
        }
        "/.simile/exchanges" => match method {
            Method::GET => match inspector.snapshot() {
                Some(exchanges) => json_response(StatusCode::OK, json!({ "exchanges": exchanges })),
                None => error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "inspection_unavailable",
                    "runtime exchange inspection is unavailable",
                ),
            },
            Method::DELETE => match inspector.clear() {
                Some(removed) => json_response(StatusCode::OK, json!({ "removed": removed })),
                None => error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "inspection_unavailable",
                    "runtime exchange inspection is unavailable",
                ),
            },
            _ => inspection_method_not_allowed("GET, DELETE, OPTIONS"),
        },
        "/.simile/state" if method == Method::GET => match state.service.inspect_state(true) {
            Ok(inspection) => json_response(StatusCode::OK, json!(inspection)),
            Err(error) => inspection_state_error(error),
        },
        "/.simile/state/reset" if method == Method::POST => {
            reset_state_response(state, request).await
        }
        "/.simile/deferred" if method == Method::GET => {
            match state.service.inspect_deferred_work() {
                Ok(inspection) => json_response(StatusCode::OK, json!(inspection)),
                Err(error) => inspection_state_error(error),
            }
        }
        _ if exchange_report_id(&path).is_some() => {
            let exchange_id = exchange_report_id(&path).unwrap_or_default();
            match method {
                Method::GET => inspector.report(exchange_id).map_or_else(
                    || {
                        error_response(
                            StatusCode::NOT_FOUND,
                            "report_not_prepared",
                            "this exchange has no prepared report",
                        )
                    },
                    |report| json_response(StatusCode::OK, json!(report)),
                ),
                Method::POST => {
                    prepare_exchange_report_response(state, inspector, exchange_id, request).await
                }
                _ => inspection_method_not_allowed("GET, POST, OPTIONS"),
            }
        }
        _ if report_submit_id(&path).is_some() => {
            let report_id = report_submit_id(&path).unwrap_or_default();
            if method == Method::POST {
                submit_exchange_report_response(state, inspector, report_id, request).await
            } else {
                inspection_method_not_allowed("POST, OPTIONS")
            }
        }
        _ if method != Method::GET => inspection_method_not_allowed("GET, OPTIONS"),
        _ => error_response(StatusCode::NOT_FOUND, "not_found", "route not found"),
    };
    with_inspection_cors(response, origin.as_deref())
}

async fn reset_state_response(state: &ServerState, request: Request<Body>) -> Response {
    let body = match decode_inspection_json::<ConfirmDestructiveActionBody>(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    if !body.confirm {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "reset_confirmation_required",
            "resetting service state requires explicit confirmation",
        );
    }
    match state.service.reset_state() {
        Ok(()) => json_response(StatusCode::OK, json!({ "reset": true })),
        Err(error) => inspection_state_error(error),
    }
}

async fn prepare_exchange_report_response(
    state: &ServerState,
    inspector: &Inspector,
    exchange_id: &str,
    request: Request<Body>,
) -> Response {
    if let Some(report) = inspector.report(exchange_id) {
        return json_response(StatusCode::OK, json!(report));
    }
    let Some(exchange) = inspector.report_capture(exchange_id) else {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "report_evidence_unavailable",
            "this exchange has no safely reproducible request evidence",
        );
    };
    let body = match decode_inspection_json::<PrepareRuntimeReportBody>(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let title = body
        .title
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| format!("Unexpected behavior in {}", exchange.operation));
    let Some(sender) = state.reporting_sender.as_ref() else {
        return reporting_unavailable();
    };
    let (reply, receiver) = oneshot::channel();
    if sender
        .send(RuntimeReportCommand::Prepare {
            exchange,
            title,
            description: body.description,
            reply,
        })
        .await
        .is_err()
    {
        return reporting_unavailable();
    }
    match tokio::time::timeout(Duration::from_secs(30), receiver).await {
        Ok(Ok(Ok(report))) => {
            inspector.set_report(exchange_id, report.clone());
            json_response(StatusCode::CREATED, json!(report))
        }
        Ok(Ok(Err(error))) => runtime_report_error_response(error),
        Ok(Err(_)) | Err(_) => reporting_unavailable(),
    }
}

async fn submit_exchange_report_response(
    state: &ServerState,
    inspector: &Inspector,
    report_id: &str,
    request: Request<Body>,
) -> Response {
    let body = match decode_inspection_json::<SubmitRuntimeReportBody>(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let contact = body.contact.filter(|contact| !contact.trim().is_empty());
    let Some(sender) = state.reporting_sender.as_ref() else {
        return reporting_unavailable();
    };
    let (reply, receiver) = oneshot::channel();
    if sender
        .send(RuntimeReportCommand::Submit {
            report: report_id.to_owned(),
            contact,
            current_version: body.current_version,
            reply,
        })
        .await
        .is_err()
    {
        return reporting_unavailable();
    }
    match tokio::time::timeout(Duration::from_secs(30), receiver).await {
        Ok(Ok(Ok(report))) => {
            inspector.update_report(report.clone());
            json_response(StatusCode::OK, json!(report))
        }
        Ok(Ok(Err(error))) => runtime_report_error_response(error),
        Ok(Err(_)) | Err(_) => reporting_unavailable(),
    }
}

async fn decode_inspection_json<T>(request: Request<Body>) -> std::result::Result<T, Response>
where
    T: for<'de> Deserialize<'de> + Default,
{
    let bytes = to_bytes(request.into_body(), 64 * 1024)
        .await
        .map_err(|_| {
            error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "inspection_request_too_large",
                "inspection request exceeds the configured limit",
            )
        })?;
    if bytes.is_empty() {
        return Ok(T::default());
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        error_response(
            StatusCode::BAD_REQUEST,
            "invalid_inspection_request",
            "inspection request body must be valid JSON",
        )
    })
}

fn exchange_report_id(path: &str) -> Option<&str> {
    path.strip_prefix("/.simile/exchanges/")
        .and_then(|path| path.strip_suffix("/report"))
        .filter(|id| !id.is_empty() && !id.contains('/'))
}

fn report_submit_id(path: &str) -> Option<&str> {
    path.strip_prefix("/.simile/reports/")
        .and_then(|path| path.strip_suffix("/submit"))
        .filter(|id| !id.is_empty() && !id.contains('/'))
}

fn inspection_method_not_allowed(allow: &'static str) -> Response {
    let mut response = error_response(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "runtime inspection does not support this method",
    );
    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static(allow));
    response
}

fn reporting_unavailable() -> Response {
    error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "reporting_unavailable",
        "runtime reporting is unavailable",
    )
}

fn runtime_report_error_response(error: RuntimeReportError) -> Response {
    let status = match error.code.as_str() {
        "report_contact_required" | "reporting_not_declared" => StatusCode::UNPROCESSABLE_ENTITY,
        "report_not_found" => StatusCode::NOT_FOUND,
        "report_verification_failed" => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    error_response(status, &error.code, &error.message)
}

fn inspection_state_error(error: simile_service::Error) -> Response {
    tracing::error!(target: "simile::inspection", %error, "could not inspect live service state");
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "inspection_unavailable",
        "runtime state inspection is unavailable",
    )
}

fn with_inspection_cors(mut response: Response, origin: Option<&str>) -> Response {
    if let Some(origin) = origin.and_then(|origin| HeaderValue::from_str(origin).ok()) {
        response
            .headers_mut()
            .insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        response
            .headers_mut()
            .insert(VARY, HeaderValue::from_static("Origin"));
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST, DELETE, OPTIONS"),
        );
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("Content-Type"),
        );
        response.headers_mut().insert(
            HeaderName::from_static("access-control-allow-private-network"),
            HeaderValue::from_static("true"),
        );
    }
    response
}

fn is_loopback_origin(origin: &str) -> bool {
    let Some(authority) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .and_then(|rest| rest.split('/').next())
    else {
        return false;
    };
    ["localhost", "127.0.0.1", "[::1]"]
        .iter()
        .any(|host| authority == *host || authority.strip_prefix(host).is_some_and(valid_port))
}

fn valid_port(suffix: &str) -> bool {
    suffix
        .strip_prefix(':')
        .is_some_and(|port| !port.is_empty() && port.parse::<u16>().is_ok())
}

fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().try_into().unwrap_or(u64::MAX)
        })
}

fn inspection_request_body(content_type: Option<&str>, bytes: &[u8]) -> InspectionMessage {
    let truncated = bytes.len() > INSPECTION_BODY_LIMIT;
    let body = (!truncated)
        .then(|| sanitize_body(content_type, bytes))
        .flatten()
        .and_then(|body| inspection_json_or_text(content_type, &body));
    InspectionMessage {
        content_type: content_type.map(str::to_owned),
        body,
        truncated,
    }
}

fn inspection_response_body(content_type: Option<&str>, bytes: &[u8]) -> InspectionMessage {
    let truncated = bytes.len() > INSPECTION_BODY_LIMIT;
    let body = (!truncated)
        .then(|| inspection_json_or_text(content_type, bytes))
        .flatten();
    InspectionMessage {
        content_type: content_type.map(str::to_owned),
        body,
        truncated,
    }
}

fn inspection_json_or_text(content_type: Option<&str>, bytes: &[u8]) -> Option<serde_json::Value> {
    if bytes.is_empty() {
        return None;
    }
    let media_type = content_type
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if media_type.is_some_and(|value| value == "application/json" || value.ends_with("+json")) {
        return serde_json::from_slice(bytes).ok();
    }
    std::str::from_utf8(bytes)
        .ok()
        .map(|value| serde_json::Value::String(value.to_owned()))
}

#[allow(clippy::too_many_arguments)]
fn emit_incident(
    state: &ServerState,
    profile: Option<String>,
    operation: &str,
    seed: u64,
    content_type: Option<String>,
    body: &[u8],
    parameters: IncomingParameters,
    error: &simile_runtime::Error,
) {
    if !error.is_reportable() {
        return;
    }
    let Some(sender) = state.reporting_sender.as_ref() else {
        return;
    };
    let Some(body) = sanitize_body(content_type.as_deref(), body) else {
        tracing::debug!(
            target: "simile::reporting",
            operation,
            content_type,
            "automatic report skipped because the request body cannot be safely sanitized"
        );
        return;
    };
    let incident = RuntimeIncident {
        profile,
        operation: operation.to_owned(),
        seed,
        content_type,
        body,
        parameters: sanitize_parameters(parameters),
        scenario: state.scenario.clone(),
        fault_profile: state.fault_profile.clone(),
        code: error.code().to_owned(),
        message: error.to_string(),
        issues: error
            .issues()
            .iter()
            .filter_map(|issue| serde_json::to_value(issue).ok())
            .collect(),
    };
    if let Err(error) = sender.try_send(RuntimeReportCommand::Automatic(incident)) {
        tracing::debug!(
            target: "simile::reporting",
            %error,
            "automatic report skipped because the reporting queue is unavailable"
        );
    }
}

fn sanitize_parameters(mut parameters: IncomingParameters) -> IncomingParameters {
    redact_parameter_map(&mut parameters.path);
    redact_parameter_map(&mut parameters.query);
    redact_parameter_map(&mut parameters.headers);
    redact_parameter_map(&mut parameters.cookies);
    parameters
}

fn redact_parameter_map(values: &mut BTreeMap<String, Vec<String>>) {
    for (name, values) in values {
        if sensitive_name(name) {
            values.clear();
            values.push("<redacted>".to_owned());
        }
    }
}

fn sanitize_body(content_type: Option<&str>, body: &[u8]) -> Option<Vec<u8>> {
    if body.is_empty() {
        return Some(Vec::new());
    }
    let media_type = content_type?.split(';').next()?.trim();
    if media_type != "application/json" && !media_type.ends_with("+json") {
        return None;
    }
    let mut value = serde_json::from_slice::<serde_json::Value>(body).ok()?;
    redact_json(&mut value);
    serde_json::to_vec(&value).ok()
}

fn redact_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            for (name, value) in object {
                if sensitive_name(name) {
                    *value = serde_json::Value::String("<redacted>".to_owned());
                } else {
                    redact_json(value);
                }
            }
        }
        serde_json::Value::Array(values) => values.iter_mut().for_each(redact_json),
        _ => {}
    }
}

fn sensitive_name(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase().replace(['-', '.'], "_");
    [
        "authorization",
        "cookie",
        "password",
        "secret",
        "token",
        "api_key",
        "apikey",
        "access_key",
        "private_key",
        "card_number",
        "account_number",
        "cvc",
        "cvv",
    ]
    .iter()
    .any(|sensitive| normalized == *sensitive || normalized.ends_with(&format!("_{sensitive}")))
}

fn incoming_parameters(
    request: &Request<Body>,
    path: BTreeMap<String, Vec<String>>,
) -> IncomingParameters {
    let mut query = BTreeMap::<String, Vec<String>>::new();
    if let Some(raw) = request.uri().query() {
        for (name, value) in form_urlencoded::parse(raw.as_bytes()) {
            query
                .entry(name.into_owned())
                .or_default()
                .push(value.into_owned());
        }
    }
    let mut headers = BTreeMap::<String, Vec<String>>::new();
    for name in request.headers().keys() {
        let values = request
            .headers()
            .get_all(name)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !values.is_empty() {
            headers.insert(name.as_str().to_ascii_lowercase(), values);
        }
    }
    let mut cookies = BTreeMap::<String, Vec<String>>::new();
    for value in request.headers().get_all(COOKIE).iter() {
        let Ok(value) = value.to_str() else {
            continue;
        };
        for pair in value.split(';') {
            let Some((name, value)) = pair.trim().split_once('=') else {
                continue;
            };
            cookies
                .entry(name.trim().to_owned())
                .or_default()
                .push(value.trim().to_owned());
        }
    }
    IncomingParameters {
        path,
        query,
        headers,
        cookies,
    }
}

async fn operation_execution_response(execution: OperationExecution, head: bool) -> Response {
    if let Some(fault) = execution.fault() {
        tracing::info!(
            target: "simile::fault",
            operation = execution.operation(),
            fault = fault.name(),
            source = ?fault.source(),
            phase = ?fault.phase(),
            "activated environmental fault"
        );
    }
    match execution.transport() {
        TransportPlan::Respond => operation_response(execution.response(), head),
        TransportPlan::RespondAfter { milliseconds } => {
            tokio::time::sleep(Duration::from_millis(*milliseconds)).await;
            operation_response(execution.response(), head)
        }
        TransportPlan::DisconnectAfterCommit => disconnect_after_commit(execution.response()),
    }
}

fn disconnect_after_commit(response: &simile_runtime::SynthesizedResponse) -> Response {
    let status =
        StatusCode::from_u16(response.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let error = std::io::Error::new(
        std::io::ErrorKind::ConnectionReset,
        "simile injected a disconnect after commit",
    );
    let body = Body::from_stream(stream::once(
        async move { Err::<Bytes, std::io::Error>(error) },
    ));
    let mut outgoing = (status, body).into_response();
    if let Some(content_type) = response
        .media_type()
        .and_then(|value| HeaderValue::from_str(value).ok())
    {
        outgoing.headers_mut().insert(CONTENT_TYPE, content_type);
    }
    if let Err(error) = apply_response_headers(&mut outgoing, response) {
        return error;
    }
    outgoing
}

fn header(headers: &HeaderMap, name: &str) -> std::result::Result<Option<String>, Response> {
    headers
        .get(name)
        .map(|value| {
            value.to_str().map(str::to_owned).map_err(|_| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_header",
                    "request contains a non-text header value",
                )
            })
        })
        .transpose()
}

fn operation_response(response: &simile_runtime::SynthesizedResponse, head: bool) -> Response {
    let status =
        StatusCode::from_u16(response.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = if head {
        Body::empty()
    } else {
        Body::from(response.bytes().to_vec())
    };
    let mut outgoing = (status, body).into_response();
    if let Some(content_type) = response.media_type() {
        let Ok(value) = HeaderValue::from_str(content_type) else {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid_response_header",
                "compiled response contains an invalid content type",
            );
        };
        outgoing.headers_mut().insert(CONTENT_TYPE, value);
    }
    if let Err(error) = apply_response_headers(&mut outgoing, response) {
        return error;
    }
    outgoing
}

fn apply_response_headers(
    outgoing: &mut Response,
    response: &simile_runtime::SynthesizedResponse,
) -> std::result::Result<(), Response> {
    for (name, value) in response.headers() {
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid_response_header",
                "compiled response contains an invalid header name",
            )
        })?;
        let value = HeaderValue::from_str(value).map_err(|_| {
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid_response_header",
                "compiled response contains an invalid header value",
            )
        })?;
        outgoing.headers_mut().append(name, value);
    }
    Ok(())
}

fn operation_error(
    runtime: &Runtime,
    operation: &str,
    error: simile_runtime::Error,
    head: bool,
) -> Response {
    let Some(cause) = service_error_cause(&error) else {
        return runtime_error(error);
    };
    match runtime.synthesize_protocol_error(operation, cause) {
        Ok(Some(response)) => {
            tracing::debug!(
                target: "simile::http",
                operation,
                status = response.status(),
                error = %error,
                issues = ?error.issues(),
                "request rejected using the emulated service contract"
            );
            operation_response(&response, head)
        }
        Ok(None) => runtime_error(error),
        Err(synthesis_error) => {
            tracing::warn!(
                target: "simile::http",
                operation,
                error = %synthesis_error,
                "could not synthesize the emulated service's declared error response"
            );
            runtime_error(error)
        }
    }
}

fn service_error_cause(error: &simile_runtime::Error) -> Option<&'static str> {
    use simile_runtime::Error as RuntimeError;

    match error {
        RuntimeError::InvalidBody { .. } => Some("invalid_body"),
        RuntimeError::MalformedJson { .. }
        | RuntimeError::MalformedBody { .. }
        | RuntimeError::InvalidParameters { .. }
        | RuntimeError::BodyRequired { .. }
        | RuntimeError::BodyNotDeclared { .. }
        | RuntimeError::EmptyReplayIdentity => Some("malformed_request"),
        RuntimeError::MissingContentType { .. }
        | RuntimeError::UnsupportedMediaType { .. }
        | RuntimeError::UnsupportedEncoding { .. } => Some("unsupported_media_type"),
        RuntimeError::IdempotencyConflict { .. } => Some("idempotency_conflict"),
        RuntimeError::AuthenticationRequired { .. } => Some("authentication_required"),
        RuntimeError::PermissionDenied { .. } => Some("permission_denied"),
        _ => None,
    }
}

fn runtime_error(error: simile_runtime::Error) -> Response {
    use simile_runtime::Error as RuntimeError;

    if let RuntimeError::InvalidParameters { issues } = &error {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({
                "error": {
                    "code": "invalid_parameters",
                    "message": "request parameters do not satisfy the operation contract",
                    "issues": issues,
                }
            }),
        );
    }

    if let RuntimeError::InvalidBody { issues } = &error {
        let message = validation_error_message(issues.len());
        return json_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({
                "error": {
                    "code": "invalid_body",
                    "message": message,
                    "issues": issues,
                }
            }),
        );
    }

    let (status, code) = match &error {
        RuntimeError::MalformedJson { .. } => (StatusCode::BAD_REQUEST, "malformed_json"),
        RuntimeError::MalformedBody { .. } => (StatusCode::BAD_REQUEST, "malformed_body"),
        RuntimeError::InvalidParameters { .. } => (StatusCode::BAD_REQUEST, "invalid_parameters"),
        RuntimeError::MissingContentType { .. }
        | RuntimeError::UnsupportedMediaType { .. }
        | RuntimeError::UnsupportedEncoding { .. } => {
            (StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type")
        }
        RuntimeError::BodyRequired { .. } | RuntimeError::BodyNotDeclared { .. } => {
            (StatusCode::BAD_REQUEST, "invalid_body")
        }
        RuntimeError::IdempotencyConflict { .. } => (StatusCode::CONFLICT, "idempotency_conflict"),
        RuntimeError::AuthenticationRequired { .. } => {
            (StatusCode::UNAUTHORIZED, "authentication_required")
        }
        RuntimeError::PermissionDenied { .. } => (StatusCode::FORBIDDEN, "permission_denied"),
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "emulation_failed"),
    };
    error_response(status, code, &error.to_string())
}

fn validation_error_message(issue_count: usize) -> String {
    if issue_count == 1 {
        "request body is invalid; fix the issue listed in `issues` and retry".to_owned()
    } else {
        format!(
            "request body is invalid; fix the {issue_count} issues listed in `issues` and retry"
        )
    }
}

fn method_not_allowed(methods: BTreeSet<&str>) -> Response {
    let mut response = error_response(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "method is not declared for this route",
    );
    let allow = methods.into_iter().collect::<Vec<_>>().join(", ");
    if let Ok(value) = HeaderValue::from_str(&allow) {
        response.headers_mut().insert(ALLOW, value);
    }
    response
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    json_response(
        status,
        json!({
            "error": {
                "code": code,
                "message": message,
            }
        }),
    )
}

fn json_response(status: StatusCode, payload: serde_json::Value) -> Response {
    (status, axum::Json(payload)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use simile_compiler::{CompileRequest, Compiler, Source};
    use simile_heuristics::Policy;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::oneshot;

    fn artifact() -> Artifact {
        let source = r#"
openapi: 3.1.0
info: { title: Orders, version: 1.0.0 }
paths:
  /orders:
    post:
      operationId: createOrder
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              additionalProperties: false
              required: [customer, items]
              properties:
                customer:
                  type: object
                  required: [email]
                  properties:
                    email: { type: string }
                items:
                  type: array
                  minItems: 1
                  items:
                    type: object
                    required: [quantity]
                    properties:
                      quantity: { type: integer, minimum: 1 }
      responses:
        '201':
          description: created
          content:
            application/json:
              schema:
                type: object
                required: [id]
                properties:
                  id: { type: string }
        '400':
          description: invalid order request
          content:
            application/json:
              schema:
                type: object
                additionalProperties: false
                required: [code, message]
                properties:
                  code: { const: invalid_order }
                  message: { const: order request rejected }
  /orders/{id}:
    parameters:
      - name: id
        in: path
        required: true
        schema: { type: string, pattern: '^ord_' }
    get:
      operationId: getOrder
      parameters:
        - name: limit
          in: query
          schema: { type: integer, minimum: 1 }
        - name: X-Trace
          in: header
          schema: { type: string, minLength: 3 }
      responses:
        '200':
          description: found
          content:
            application/json:
              schema:
                type: object
                required: [id]
                properties:
                  id: { type: string }
  /orders/recent:
    get:
      operationId: recentOrders
      responses:
        '204': { description: empty }
  /file-links/{id}/open:
    get:
      operationId: openFileLink
      responses:
        '303':
          description: open file
          headers:
            Location:
              schema: { type: string, format: uri }
              example: https://files.example.test/download/file_1234
"#;
        compile_artifact(source)
    }

    fn artifact_without_client_error() -> Artifact {
        compile_artifact(
            r#"
openapi: 3.1.0
info: { title: Orders, version: 1.0.0 }
paths:
  /orders:
    post:
      operationId: createOrder
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              required: [customer]
              properties:
                customer: { type: string }
      responses:
        '201':
          description: created
          content:
            application/json:
              schema:
                type: object
                required: [id]
                properties:
                  id: { type: string }
"#,
        )
    }

    fn artifact_with_faults() -> Artifact {
        compile_artifact_with_behavior(
            r#"
openapi: 3.1.0
info: { title: Orders, version: 1.0.0 }
paths:
  /orders:
    post:
      operationId: createOrder
      responses:
        '201':
          description: created
          content:
            application/json:
              schema:
                type: object
                required: [id]
                properties:
                  id: { type: string }
"#,
            r#"
fault "slow_response" {
  phase = "before_response"
  delay { milliseconds = 20 }
}

fault "response_lost" {
  phase = "after_commit"
  disconnect {}
}

fault_profile "always_slow" {
  probability       = 1
  faults            = ["slow_response"]
  operations        = ["createOrder"]
  max_consecutive   = 1
  cooldown_requests = 1
  seed              = 7
}

scenario "slow" {
  inject "createOrder" { fault = "slow_response" }
}

scenario "lost" {
  inject "createOrder" { fault = "response_lost" }
}
"#,
        )
    }

    fn artifact_with_authentication() -> Artifact {
        compile_artifact_with_behavior(
            r#"
openapi: 3.1.0
info: { title: Secured orders, version: 1.0.0 }
paths:
  /private:
    get:
      operationId: privateOrders
      security:
        - merchantKey: []
      responses:
        '200':
          description: allowed
          content:
            application/json:
              schema:
                type: object
                required: [allowed]
                properties:
                  allowed: { const: true }
components:
  securitySchemes:
    merchantKey:
      type: apiKey
      in: header
      name: X-Merchant-Key
"#,
            r#"
principal "merchant" {
  credential "merchantKey" {}
}

profile "merchant" {
  principal = principal.merchant
  credential "merchantKey" { value = "sim_test_merchant" }
}
"#,
        )
    }

    fn compile_artifact(source: &str) -> Artifact {
        compile_artifact_with_behavior(source, "")
    }

    fn compile_artifact_with_behavior(source: &str, behavior: &str) -> Artifact {
        let compiled = Compiler::new()
            .compile(CompileRequest {
                openapi: Source::new("orders.yml", source),
                modules: (!behavior.is_empty())
                    .then(|| Source::new("faults.hcl", behavior))
                    .into_iter()
                    .collect(),
                heuristic_policy: Policy::Standard,
            })
            .unwrap_or_else(|error| panic!("{error}"));
        let encoded = simile_artifact::encode(&compiled).unwrap_or_else(|error| panic!("{error}"));
        simile_artifact::decode(&encoded).unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn formats_a_single_safe_access_log_line() {
        let method = Method::POST;
        let line = AccessLine {
            method: &method,
            path: "/orders",
            status: StatusCode::CREATED,
            origin_ip: Some(IpAddr::from([127, 0, 0, 1])),
            user_agent: Some("simile-test/1.0"),
            request_bytes: Some(47),
            request_content_type: Some("application/json"),
            response_bytes: Some(58),
            response_content_type: Some("application/json"),
            elapsed: Duration::from_micros(321),
        };

        assert_eq!(
            line.to_string(),
            "POST /orders 201 origin=127.0.0.1 user-agent=\"simile-test/1.0\" request=47B \
             request-type=\"application/json\" response=58B response-type=\"application/json\" \
             elapsed=321us"
        );
    }

    #[test]
    fn formats_validation_error_summaries() {
        assert_eq!(
            validation_error_message(1),
            "request body is invalid; fix the issue listed in `issues` and retry"
        );
        assert_eq!(
            validation_error_message(3),
            "request body is invalid; fix the 3 issues listed in `issues` and retry"
        );
    }

    #[test]
    fn prefers_literal_routes_and_reports_allowed_methods() {
        let artifact = artifact();
        let routes = RouteTable::new(&artifact);

        assert!(matches!(
            routes.find(&Method::GET, "/orders/recent"),
            Match::Operation { operation: "GET /orders/recent", path } if path.is_empty()
        ));
        assert!(matches!(
            routes.find(&Method::GET, "/orders/ord_123"),
            Match::Operation { operation: "GET /orders/{id}", path }
                if path.get("id") == Some(&vec!["ord_123".to_owned()])
        ));
        assert!(matches!(
            routes.find(&Method::POST, "/orders/ord_123"),
            Match::MethodNotAllowed(methods) if methods == BTreeSet::from(["GET"])
        ));
        assert!(matches!(
            routes.find(&Method::GET, "/customers/cus_123"),
            Match::NotFound
        ));
    }

    #[tokio::test]
    async fn binds_to_an_ephemeral_loopback_port() {
        let server = Server::bind(SocketAddr::from(([127, 0, 0, 1], 0)), artifact())
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        assert_ne!(server.local_addr().port(), 0);
        assert!(server.local_addr().ip().is_loopback());
    }

    #[tokio::test]
    async fn rejects_an_unknown_scenario_before_binding() {
        let result = Server::bind_with_scenario(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            artifact(),
            Some("missing".to_owned()),
        )
        .await;

        assert!(matches!(result, Err(Error::UnknownScenario(name)) if name == "missing"));
    }

    #[tokio::test]
    async fn rejects_an_unknown_fault_profile_before_binding() {
        let result = Server::bind_with_options(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            artifact_with_faults(),
            ServerOptions {
                scenario: None,
                fault_profile: Some("missing".to_owned()),
                reporting_sender: None,
            },
        )
        .await;

        assert!(matches!(result, Err(Error::UnknownFaultProfile(name)) if name == "missing"));
    }

    #[tokio::test]
    async fn reports_the_active_fault_profiles_seed() {
        let server = Server::bind_with_options(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            artifact_with_faults(),
            ServerOptions {
                scenario: None,
                fault_profile: Some("always_slow".to_owned()),
                reporting_sender: None,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(server.fault_seed(), Some(7));
    }

    #[tokio::test]
    async fn enacts_delay_and_disconnect_transport_plans() {
        let artifact = artifact_with_faults();
        let runtime = Runtime::new(&artifact);
        let call = OperationCall {
            operation: "POST /orders",
            content_type: None,
            body: b"",
        };
        let delayed = runtime
            .execute_with_options(
                call,
                ExecutionOptions {
                    scenario: Some("slow"),
                    ..ExecutionOptions::default()
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let started = Instant::now();
        let response = operation_execution_response(delayed, false).await;

        assert_eq!(response.status(), StatusCode::CREATED);
        assert!(started.elapsed() >= Duration::from_millis(10));

        let disconnected = runtime
            .execute_with_options(
                call,
                ExecutionOptions {
                    scenario: Some("lost"),
                    ..ExecutionOptions::default()
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let response = operation_execution_response(disconnected, false).await;

        assert_eq!(response.status(), StatusCode::CREATED);
        assert!(to_bytes(response.into_body(), 1_024).await.is_err());
    }

    async fn serve_raw_with_artifact(artifact: Artifact, request: &[u8]) -> String {
        let server = Server::bind(SocketAddr::from(([127, 0, 0, 1], 0)), artifact)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let address = server.local_addr();
        let (shutdown, signal) = oneshot::channel::<()>();
        let task = tokio::spawn(server.run_until(async {
            drop(signal.await);
        }));
        let response = send_raw(address, request).await;
        let _ = shutdown.send(());
        task.await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|error| panic!("{error}"));
        response
    }

    async fn send_raw(address: SocketAddr, request: &[u8]) -> String {
        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        stream
            .write_all(request)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        String::from_utf8(response).unwrap_or_else(|error| panic!("{error}"))
    }

    async fn serve_raw(request: &[u8]) -> String {
        serve_raw_with_artifact(artifact(), request).await
    }

    #[tokio::test]
    async fn serves_a_request_through_the_operation_runtime() {
        let response = serve_raw(
            b"GET /orders/ord_123 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await;

        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("content-type: application/json"));
        assert!(response.contains(r#""id":"#));
    }

    #[tokio::test]
    async fn exposes_live_inspection_only_to_loopback_browser_origins() {
        let server = Server::bind(SocketAddr::from(([127, 0, 0, 1], 0)), artifact())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let address = server.local_addr();
        let (shutdown, signal) = oneshot::channel::<()>();
        let task = tokio::spawn(server.run_until(async {
            drop(signal.await);
        }));

        let body = br#"{"customer":{"email":"ada@example.test"},"items":[{"quantity":1}]}"#;
        let request = format!(
            "POST /orders HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            String::from_utf8_lossy(body)
        );
        let operation = send_raw(address, request.as_bytes()).await;
        assert!(operation.starts_with("HTTP/1.1 201 Created\r\n"));

        let exchanges = send_raw(
            address,
            b"GET /.simile/exchanges HTTP/1.1\r\nHost: localhost\r\nOrigin: http://127.0.0.1:4175\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(exchanges.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(exchanges.contains("access-control-allow-origin: http://127.0.0.1:4175\r\n"));
        assert!(exchanges.contains(r#""operation":"POST /orders""#));
        assert!(exchanges.contains(r#""status":201"#));
        assert!(exchanges.contains(r#""email":"ada@example.test""#));

        let state = send_raw(
            address,
            b"GET /.simile/state HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(state.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(state.contains(r#""resources":"#));
        assert!(state.contains(r#""events":"#));

        let deferred = send_raw(
            address,
            b"GET /.simile/deferred HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(deferred.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(deferred.contains(r#""pending":"#));
        assert!(deferred.contains(r#""deadLetters":"#));

        let unconfirmed_reset = send_raw(
            address,
            b"POST /.simile/state/reset HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
        )
        .await;
        assert!(unconfirmed_reset.starts_with("HTTP/1.1 422 Unprocessable Entity\r\n"));
        assert!(unconfirmed_reset.contains("reset_confirmation_required"));

        let reset = send_raw(
            address,
            b"POST /.simile/state/reset HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{\"confirm\":true}",
        )
        .await;
        assert!(reset.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(reset.contains(r#""reset":true"#));

        let reset_state = send_raw(
            address,
            b"GET /.simile/state HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(reset_state.contains(r#""events":[]"#));

        let cleared = send_raw(
            address,
            b"DELETE /.simile/exchanges HTTP/1.1\r\nHost: localhost\r\nOrigin: http://127.0.0.1:4175\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(cleared.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(cleared.contains(r#""removed":1"#));

        let empty_exchanges = send_raw(
            address,
            b"GET /.simile/exchanges HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(empty_exchanges.contains(r#""exchanges":[]"#));

        let forbidden = send_raw(
            address,
            b"GET /.simile/inspection HTTP/1.1\r\nHost: localhost\r\nOrigin: https://example.com\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(forbidden.starts_with("HTTP/1.1 403 Forbidden\r\n"));
        assert!(forbidden.contains("inspection_origin_forbidden"));

        let _ = shutdown.send(());
        task.await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|error| panic!("{error}"));
    }

    #[tokio::test]
    async fn does_not_expose_inspection_on_a_non_loopback_listener() {
        let server = Server::bind(SocketAddr::from(([0, 0, 0, 0], 0)), artifact())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let address = SocketAddr::from(([127, 0, 0, 1], server.local_addr().port()));
        let (shutdown, signal) = oneshot::channel::<()>();
        let task = tokio::spawn(server.run_until(async {
            drop(signal.await);
        }));

        let response = send_raw(
            address,
            b"GET /.simile/inspection HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 404 Not Found\r\n"));

        let _ = shutdown.send(());
        task.await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|error| panic!("{error}"));
    }

    #[tokio::test]
    async fn serves_declared_redirect_headers() {
        let response = serve_raw(
            b"GET /file-links/link_123/open HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await;

        assert!(response.starts_with("HTTP/1.1 303 See Other\r\n"));
        assert!(response.contains("location: https://files.example.test/download/file_1234\r\n"));
    }

    #[tokio::test]
    async fn enforces_declared_authentication_before_operation_execution() {
        let missing = serve_raw_with_artifact(
            artifact_with_authentication(),
            b"GET /private HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(missing.starts_with("HTTP/1.1 401 Unauthorized\r\n"));
        assert!(missing.contains(r#""code":"authentication_required""#));

        let accepted = serve_raw_with_artifact(
            artifact_with_authentication(),
            b"GET /private HTTP/1.1\r\nHost: localhost\r\nX-Merchant-Key: sim_test_merchant\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(accepted.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(accepted.contains(r#""allowed":true"#));
    }

    #[tokio::test]
    async fn validates_path_query_and_header_parameters_before_execution() {
        let invalid = serve_raw(
            b"GET /orders/not-an-order?limit=0 HTTP/1.1\r\nHost: localhost\r\nX-Trace: ok\r\nConnection: close\r\n\r\n",
        )
        .await;
        let (_, body) = invalid
            .split_once("\r\n\r\n")
            .unwrap_or_else(|| panic!("response should contain a body"));
        let payload: serde_json::Value =
            serde_json::from_str(body).unwrap_or_else(|error| panic!("{error}"));

        assert!(invalid.starts_with("HTTP/1.1 400 Bad Request\r\n"));
        assert_eq!(payload["error"]["code"], "invalid_parameters");
        assert_eq!(payload["error"]["issues"].as_array().map(Vec::len), Some(3));

        let valid = serve_raw(
            b"GET /orders/ord_123?limit=2 HTTP/1.1\r\nHost: localhost\r\nX-Trace: trace-1\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(valid.starts_with("HTTP/1.1 200 OK\r\n"));
    }

    #[tokio::test]
    async fn returns_the_service_declared_validation_error() {
        let body = br#"{"customer":{},"items":[{"quantity":0}],"debug":true}"#;
        let request = format!(
            "POST /orders HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            String::from_utf8_lossy(body)
        );
        let response = serve_raw(request.as_bytes()).await;
        let (_, body) = response
            .split_once("\r\n\r\n")
            .unwrap_or_else(|| panic!("response should contain headers and a body"));
        let payload: serde_json::Value =
            serde_json::from_str(body).unwrap_or_else(|error| panic!("{error}"));

        assert!(response.starts_with("HTTP/1.1 400 Bad Request\r\n"));
        assert_eq!(
            payload,
            json!({
                "code": "invalid_order",
                "message": "order request rejected"
            })
        );
    }

    #[tokio::test]
    async fn falls_back_to_simile_diagnostics_without_a_declared_client_error() {
        let body = br#"{}"#;
        let request = format!(
            "POST /orders HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            String::from_utf8_lossy(body)
        );
        let response =
            serve_raw_with_artifact(artifact_without_client_error(), request.as_bytes()).await;
        let (_, body) = response
            .split_once("\r\n\r\n")
            .unwrap_or_else(|| panic!("response should contain headers and a body"));
        let payload: serde_json::Value =
            serde_json::from_str(body).unwrap_or_else(|error| panic!("{error}"));

        assert!(response.starts_with("HTTP/1.1 422 Unprocessable Entity\r\n"));
        assert_eq!(payload["error"]["code"], "invalid_body");
        assert_eq!(
            payload["error"]["issues"],
            json!([{
                "path": "/customer",
                "rule": "required",
                "message": "required property `customer` is missing"
            }])
        );
    }

    #[test]
    fn sanitizes_sensitive_incident_evidence_without_flattening_the_request() {
        let body = br#"{
          "customer": {"name": "Ada", "api_key": "sk_live_secret"},
          "password_reset_token": "reset-secret",
          "items": [{"reference": "sku_123"}]
        }"#;
        let sanitized = sanitize_body(Some("application/problem+json; charset=utf-8"), body)
            .unwrap_or_else(|| panic!("JSON incident body must be sanitizable"));
        let value: serde_json::Value = serde_json::from_slice(&sanitized)
            .unwrap_or_else(|error| panic!("sanitized body must remain JSON: {error}"));

        assert_eq!(value["customer"]["name"], "Ada");
        assert_eq!(value["customer"]["api_key"], "<redacted>");
        assert_eq!(value["password_reset_token"], "<redacted>");
        assert_eq!(value["items"][0]["reference"], "sku_123");

        let parameters = sanitize_parameters(IncomingParameters {
            path: BTreeMap::from([("access_token".to_owned(), vec!["path-secret".to_owned()])]),
            query: BTreeMap::from([("page".to_owned(), vec!["2".to_owned()])]),
            headers: BTreeMap::from([(
                "authorization".to_owned(),
                vec!["Bearer secret".to_owned()],
            )]),
            cookies: BTreeMap::from([("session_token".to_owned(), vec!["cookie".to_owned()])]),
        });
        assert_eq!(parameters.path["access_token"], ["<redacted>"]);
        assert_eq!(parameters.query["page"], ["2"]);
        assert_eq!(parameters.headers["authorization"], ["<redacted>"]);
        assert_eq!(parameters.cookies["session_token"], ["<redacted>"]);
    }

    #[test]
    fn declines_opaque_request_bodies_for_automatic_reports() {
        assert!(sanitize_body(Some("application/octet-stream"), b"secret bytes").is_none());
        assert!(sanitize_body(None, b"secret bytes").is_none());
    }
}
