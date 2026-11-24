#![forbid(unsafe_code)]

mod access_log;
mod delivery;
mod exchange_store;
mod inspection;
mod process;
mod proxy;
mod response;
mod routing;

pub use exchange_store::Error as ExchangeHistoryError;
pub use process::CommandResult;
pub use proxy::{ProxyOptions, ProxyServer};

use access_log::access_log;
use inspection::{
    emit_incident, inspection_request_body, inspection_response, inspection_response_body,
    redact_json, sanitize_body, sanitize_parameters, unix_time_millis,
};
use response::{
    error_response, header, method_not_allowed, operation_error, operation_execution_response,
};
use routing::{Match, RouteTable};

use std::collections::BTreeMap;
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Instant;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request, StatusCode};
use axum::middleware;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use simile_artifact::Artifact;
use simile_runtime::{
    ExecutionOptions, IncomingParameters, OperationCall, ReplayIdentity, validate_parameters,
};
use simile_service::Service;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

const DEFAULT_BODY_LIMIT: usize = 8 * 1024 * 1024;
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

    #[error("could not initialize exchange history: {0}")]
    ExchangeHistory(#[from] ExchangeHistoryError),

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

#[derive(Clone, Debug, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RuntimeExpectedResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
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

struct Inspector {
    next_exchange: AtomicU64,
    exchanges: exchange_store::ExchangeStore,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RecordedExchange {
    exchange: InspectionExchange,
    report_capture: Option<RuntimeExchangeReport>,
    report: Option<RuntimeReportSummary>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
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
        Self::bind_service(address, service, options, InspectionStorage::Memory).await
    }

    /// Bind a service and persist its Stack and sanitized exchange history.
    pub async fn bind_durable_with_options(
        address: SocketAddr,
        artifact: Artifact,
        state_path: impl AsRef<Path>,
        options: ServerOptions,
    ) -> Result<Self> {
        let state_path = state_path.as_ref();
        let service = Service::durable(artifact, state_path)?;
        Self::bind_service(
            address,
            service,
            options,
            InspectionStorage::Durable(exchange_history_path(state_path)),
        )
        .await
    }

    /// Bind a service whose state is owned by the supplied supervisor.
    pub async fn bind_service_with_options(
        address: SocketAddr,
        service: Service,
        options: ServerOptions,
    ) -> Result<Self> {
        Self::bind_service(address, service, options, InspectionStorage::Memory).await
    }

    async fn bind_service(
        address: SocketAddr,
        service: Service,
        options: ServerOptions,
        inspection_storage: InspectionStorage,
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
        let inspector = if address.ip().is_loopback() {
            Some(Arc::new(match inspection_storage {
                InspectionStorage::Memory => Inspector::memory(),
                InspectionStorage::Durable(path) => Inspector::durable(path)?,
            }))
        } else {
            None
        };
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
    let exchange_identity = state
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
            if let (Some(inspector), Some((exchange_id, exchange_sequence))) =
                (state.inspector.as_ref(), exchange_identity)
            {
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
                inspector.record(
                    exchange_sequence,
                    RecordedExchange {
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
                    },
                );
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

enum InspectionStorage {
    Memory,
    Durable(PathBuf),
}

/// Return the durable exchange-history database beside one service state database.
#[must_use]
pub fn exchange_history_path(service_state_path: impl AsRef<Path>) -> PathBuf {
    service_state_path.as_ref().with_extension("exchanges.redb")
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

#[cfg(test)]
mod tests;
