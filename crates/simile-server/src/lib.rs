#![forbid(unsafe_code)]

mod access_log;
mod delivery;
mod exchange_store;
mod inspection;
mod process;
mod proxy;
mod response;

pub use exchange_store::Error as ExchangeHistoryError;
pub use process::CommandResult;
pub use proxy::{ProxyOptions, ProxyServer};

use access_log::access_log;
use futures_util::stream;
use inspection::{
    inspection_request_body, inspection_response, inspection_response_body, redact_json,
    sanitize_body, sanitize_parameters, unix_time_millis,
};
use simile_http::RouteTable;
use simile_runtime::{IncomingParameters, TransportPlan};

use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::extract::State;
use axum::http::Request;
use axum::middleware;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use simile_artifact::Artifact;
use simile_service::Service;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

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

pub use simile_http::RuntimeIncident;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RuntimeExchangeReport {
    pub exchange_id: String,
    pub profile: Option<String>,
    pub operation: String,
    pub response_seed: u64,
    #[serde(default)]
    pub logical_time_milliseconds: Option<i64>,
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
    AutomaticExchange(RuntimeExchangeReport),
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
    response_seed: Option<u64>,
    incident_sender: Option<mpsc::Sender<RuntimeIncident>>,
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
        let http_options = simile_http::HttpOptions {
            scenario: options.scenario.clone(),
            fault_profile: options.fault_profile.clone(),
            ..Default::default()
        };
        Self::bind_service_http(
            address,
            service,
            options,
            inspection_storage,
            http_options,
            None,
        )
        .await
    }

    /// Bind with explicit HTTP controls for reproducible embeddings and parity fixtures.
    pub async fn bind_service_with_http_options(
        address: SocketAddr,
        service: Service,
        options: simile_http::HttpOptions,
        incident_sender: Option<mpsc::Sender<RuntimeIncident>>,
    ) -> Result<Self> {
        let server_options = ServerOptions {
            scenario: options.scenario.clone(),
            fault_profile: options.fault_profile.clone(),
            reporting_sender: None,
        };
        Self::bind_service_http(
            address,
            service,
            server_options,
            InspectionStorage::Memory,
            options,
            incident_sender,
        )
        .await
    }

    async fn bind_service_http(
        address: SocketAddr,
        service: Service,
        options: ServerOptions,
        inspection_storage: InspectionStorage,
        http_options: simile_http::HttpOptions,
        incident_sender: Option<mpsc::Sender<RuntimeIncident>>,
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
            body_limit: http_options.body_limit,
            response_seed: http_options.response_seed,
            incident_sender,
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
    let (parts, body) = request.into_parts();
    let body = match to_bytes(body, state.body_limit).await {
        Ok(bytes) => simile_http::RequestBody::Bytes(bytes.to_vec()),
        Err(_) => simile_http::RequestBody::TooLarge,
    };
    let mut exchange = simile_http::handle_request(
        &state.service,
        &state.routes,
        Request::from_parts(parts, body),
        &simile_http::HttpOptions {
            body_limit: state.body_limit,
            scenario: state.scenario.clone(),
            fault_profile: state.fault_profile.clone(),
            response_seed: state.response_seed,
        },
    );
    if let Some(incident) = exchange.incident.take() {
        if let Some(sender) = &state.incident_sender {
            let _ = sender.try_send(incident.clone());
        }
        if let Some(sender) = &state.reporting_sender {
            if let Err(error) = sender.try_send(RuntimeReportCommand::Automatic(incident)) {
                tracing::debug!(target: "simile::reporting", %error, "automatic report skipped because the reporting queue is unavailable");
            }
        }
    }
    if let Some(fault) = &exchange.fault {
        tracing::info!(target: "simile::fault", operation = exchange.operation, fault = fault.name(), source = ?fault.source(), phase = ?fault.phase(), "activated environmental fault");
    }
    let capture = exchange.capture.take();
    let operation = exchange.operation.clone();
    let profile = exchange.profile.clone();
    let response_seed = exchange.seed;
    let exchange_identity = capture.as_ref().and_then(|_| {
        state
            .inspector
            .as_ref()
            .map(|inspector| inspector.next_id(started_at_ms))
    });
    let response = send_exchange(exchange).await;
    if let (Some(capture), Some(operation)) = (capture, operation) {
        let request_message =
            inspection_request_body(capture.content_type.as_deref(), &capture.body);
        let response_message = inspection_response_body(
            capture.response_content_type.as_deref(),
            &capture.response_bytes,
        );
        let mut expected_body = capture.response_body;
        if let Some(body) = expected_body.as_mut() {
            redact_json(body);
        }
        let report_capture =
            sanitize_body(capture.content_type.as_deref(), &capture.body).map(|body| {
                RuntimeExchangeReport {
                    exchange_id: exchange_identity
                        .as_ref()
                        .map_or_else(String::new, |(id, _)| id.clone()),
                    profile: profile.clone(),
                    operation: operation.clone(),
                    response_seed,
                    logical_time_milliseconds: Some(capture.logical_time_milliseconds),
                    content_type: capture.content_type,
                    body,
                    parameters: sanitize_parameters(capture.parameters),
                    outcome: capture.outcome.clone(),
                    scenario: state.scenario.clone(),
                    fault_profile: state.fault_profile.clone(),
                    expected: RuntimeExpectedResponse {
                        status: capture.status,
                        content_type: capture.response_content_type,
                        body: expected_body,
                    },
                }
            });
        if capture.effect_failed {
            if let (Some(sender), Some(report)) = (&state.reporting_sender, &report_capture) {
                if let Err(error) =
                    sender.try_send(RuntimeReportCommand::AutomaticExchange(report.clone()))
                {
                    tracing::debug!(target: "simile::reporting", %error, "automatic effect report skipped because the reporting queue is unavailable");
                }
            }
        }
        if let (Some(inspector), Some((id, sequence))) = (&state.inspector, exchange_identity) {
            inspector.record(
                sequence,
                RecordedExchange {
                    exchange: InspectionExchange {
                        id,
                        operation,
                        method: method.to_string(),
                        path: uri.path().to_owned(),
                        started_at_ms,
                        duration_micros: started
                            .elapsed()
                            .as_micros()
                            .try_into()
                            .unwrap_or(u64::MAX),
                        status: capture.status,
                        outcome: capture.outcome,
                        profile,
                        request: request_message,
                        response: response_message,
                    },
                    report_capture,
                    report: None,
                },
            );
        }
    }
    response
}

async fn send_exchange(exchange: simile_http::HttpExchange) -> Response {
    let (parts, bytes) = exchange.response.into_parts();
    let body = match exchange.transport {
        TransportPlan::Respond => Body::from(bytes),
        TransportPlan::RespondAfter { milliseconds } => {
            tokio::time::sleep(Duration::from_millis(milliseconds)).await;
            Body::from(bytes)
        }
        TransportPlan::DisconnectAfterCommit => {
            let error = std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "simile injected a disconnect after commit",
            );
            Body::from_stream(stream::once(
                async move { Err::<Bytes, std::io::Error>(error) },
            ))
        }
    };
    Response::from_parts(parts, body)
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

#[cfg(test)]
mod tests;
