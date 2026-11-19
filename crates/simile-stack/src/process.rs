use std::io::Write as _;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::Parser;
use serde::{Deserialize, Serialize};
use simile_spec::ResourceKind;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::{
    AdvanceReport, CommandEnvelope, CommandReceipt, ComponentManifest, ErrorCode, EventQuery,
    JournalEntry, LogicalTime, PROTOCOL_VERSION, ResourceAddress, ResourceError, Stack,
    StackSnapshot,
};

type ProcessResult<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Debug, Parser)]
#[command(about = "Run one deterministic Simile Stack resource component")]
struct Arguments {
    #[arg(long, default_value_t = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))]
    listen: SocketAddr,
    #[arg(long, required = true)]
    resource: Vec<String>,
    #[arg(long, default_value_t = 0)]
    start_time_ms: i64,
}

#[derive(Clone)]
struct ProcessState {
    kind: ResourceKind,
    stack: Arc<Mutex<Stack>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Readiness {
    protocol_version: u16,
    kind: ResourceKind,
    address: SocketAddr,
}

#[derive(Debug, Serialize)]
struct Health {
    status: &'static str,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdvanceRequest {
    milliseconds: u64,
}

#[derive(Debug)]
struct ApiError(ResourceError);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.0.code {
            ErrorCode::NotFound => StatusCode::NOT_FOUND,
            ErrorCode::Conflict => StatusCode::CONFLICT,
            ErrorCode::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::InvalidCommand
            | ErrorCode::InvalidValue
            | ErrorCode::KindMismatch
            | ErrorCode::UndeclaredResource => StatusCode::BAD_REQUEST,
        };
        (status, Json(self.0)).into_response()
    }
}

impl From<ResourceError> for ApiError {
    fn from(error: ResourceError) -> Self {
        Self(error)
    }
}

pub async fn run_component(kind: ResourceKind) -> ProcessResult<()> {
    let arguments = Arguments::parse();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();

    let resources = arguments
        .resource
        .into_iter()
        .map(|name| ResourceAddress::new(kind, name))
        .collect::<crate::Result<Vec<_>>>()?;
    let stack = Stack::builder(LogicalTime::from_unix_millis(arguments.start_time_ms))
        .resources(resources)?
        .build()?;
    let state = ProcessState {
        kind,
        stack: Arc::new(Mutex::new(stack)),
    };
    let application = Router::new()
        .route("/v1/manifest", get(manifest))
        .route("/v1/health", get(health))
        .route("/v1/commands", post(execute))
        .route("/v1/events", post(events))
        .route("/v1/time/advance", post(advance))
        .route("/v1/reset", post(reset))
        .route("/v1/snapshot", get(snapshot))
        .route("/v1/restore", post(restore))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(arguments.listen).await?;
    let address = listener.local_addr()?;
    let readiness = Readiness {
        protocol_version: PROTOCOL_VERSION,
        kind,
        address,
    };
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    serde_json::to_writer(&mut stdout, &readiness)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    info!(%kind, %address, "stack component ready");

    axum::serve(listener, application).await?;
    Ok(())
}

async fn manifest(State(state): State<ProcessState>) -> Json<ComponentManifest> {
    Json(ComponentManifest {
        protocol_version: PROTOCOL_VERSION,
        kind: state.kind,
        capabilities: capabilities(state.kind)
            .iter()
            .map(|capability| (*capability).to_owned())
            .collect(),
    })
}

async fn health() -> Json<Health> {
    Json(Health { status: "ready" })
}

async fn execute(
    State(state): State<ProcessState>,
    Json(envelope): Json<CommandEnvelope>,
) -> std::result::Result<Json<CommandReceipt>, ApiError> {
    if envelope.resource.kind != state.kind {
        return Err(ApiError(ResourceError::new(
            ErrorCode::KindMismatch,
            "the command was sent to the wrong component process",
        )));
    }
    let receipt = lock(&state)?.execute_envelope(envelope)?;
    Ok(Json(receipt))
}

async fn events(
    State(state): State<ProcessState>,
    Json(query): Json<EventQuery>,
) -> std::result::Result<Json<Vec<JournalEntry>>, ApiError> {
    let entries = lock(&state)?
        .journal()
        .query(&query)
        .into_iter()
        .cloned()
        .collect();
    Ok(Json(entries))
}

async fn advance(
    State(state): State<ProcessState>,
    Json(request): Json<AdvanceRequest>,
) -> std::result::Result<Json<AdvanceReport>, ApiError> {
    Ok(Json(lock(&state)?.advance(request.milliseconds)?))
}

async fn reset(State(state): State<ProcessState>) -> std::result::Result<StatusCode, ApiError> {
    lock(&state)?.reset();
    Ok(StatusCode::NO_CONTENT)
}

async fn snapshot(
    State(state): State<ProcessState>,
) -> std::result::Result<Json<StackSnapshot>, ApiError> {
    Ok(Json(lock(&state)?.snapshot()?))
}

async fn restore(
    State(state): State<ProcessState>,
    Json(snapshot): Json<StackSnapshot>,
) -> std::result::Result<StatusCode, ApiError> {
    let restored = Stack::from_snapshot(snapshot);
    if restored
        .resources()
        .iter()
        .any(|resource| resource.kind != state.kind)
    {
        return Err(ApiError(ResourceError::new(
            ErrorCode::KindMismatch,
            "the snapshot contains resources for another component kind",
        )));
    }
    *lock(&state)? = restored;
    Ok(StatusCode::NO_CONTENT)
}

fn lock(state: &ProcessState) -> std::result::Result<MutexGuard<'_, Stack>, ApiError> {
    state.stack.lock().map_err(|_| {
        ApiError(ResourceError::new(
            ErrorCode::Unavailable,
            "the component state is unavailable",
        ))
    })
}

const fn capabilities(kind: ResourceKind) -> &'static [&'static str] {
    match kind {
        ResourceKind::Db => &["get", "put", "delete", "query", "transact"],
        ResourceKind::Cache => &["get", "set", "delete", "increment", "compare_and_set"],
        ResourceKind::Queue => &[
            "publish",
            "receive",
            "ack",
            "nack",
            "extend_visibility",
            "size",
        ],
        ResourceKind::Mail => &["send", "get", "list", "deliver", "bounce", "reject"],
        ResourceKind::ObjectStore => &["put", "get", "head", "delete", "list"],
        ResourceKind::EventBus => &["publish", "read", "commit", "checkpoint"],
        ResourceKind::Scheduler => &["schedule", "cancel", "get", "list", "run_due"],
        ResourceKind::Http => &["register", "call", "requests", "clear"],
    }
}
