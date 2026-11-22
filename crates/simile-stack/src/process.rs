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

use crate::store::default_component_state_path;
use crate::{
    AdvanceReport, CommandEnvelope, CommandReceipt, ComponentManifest, ErrorCode, EventQuery,
    JournalEntry, LogicalTime, PROTOCOL_VERSION, RedbStateStore, ResourceAddress, ResourceError,
    Stack, StackSnapshot, StateStore, StateStoreError,
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
    store: Arc<dyn StateStore>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Readiness {
    protocol_version: u16,
    kind: ResourceKind,
    address: SocketAddr,
    persistent: bool,
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

impl From<StateStoreError> for ApiError {
    fn from(error: StateStoreError) -> Self {
        Self(ResourceError::new(
            ErrorCode::Unavailable,
            error.to_string(),
        ))
    }
}

impl ProcessState {
    /// Applies one coherent state transition and checkpoints it before the
    /// caller can observe success or a domain failure. A storage failure rolls
    /// the in-memory state back to the exact pre-transition snapshot.
    fn transition<T>(
        &self,
        operation: impl FnOnce(&mut Stack) -> crate::Result<T>,
    ) -> std::result::Result<T, ApiError> {
        let mut stack = lock(self)?;
        let before = stack.snapshot()?;
        let result = operation(&mut stack);
        let after = match stack.snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                *stack = Stack::from_snapshot(before);
                return Err(ApiError(error));
            }
        };
        if let Err(error) = self.store.save(&after) {
            *stack = Stack::from_snapshot(before);
            return Err(ApiError::from(error));
        }
        result.map_err(ApiError::from)
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
    let initial_stack = Stack::builder(LogicalTime::from_unix_millis(arguments.start_time_ms))
        .resources(resources)?
        .build()?;
    let state_path = default_component_state_path(kind, initial_stack.resources())?;
    let store: Arc<dyn StateStore> = Arc::new(RedbStateStore::open(&state_path)?);
    let stack = match store.load()? {
        Some(snapshot) => {
            let restored = Stack::from_snapshot(snapshot);
            if restored.resources() != initial_stack.resources() {
                return Err(ResourceError::new(
                    ErrorCode::InvalidValue,
                    "persisted resources do not match the resources requested for this process",
                )
                .into());
            }
            restored
        }
        None => {
            store.save(&initial_stack.snapshot()?)?;
            initial_stack
        }
    };
    let persistent = store.is_durable();
    let state = ProcessState {
        kind,
        stack: Arc::new(Mutex::new(stack)),
        store,
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
        persistent,
    };
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    serde_json::to_writer(&mut stdout, &readiness)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    info!(%kind, %address, persistent, state_file = %state_path.display(), "stack component ready");

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
    let receipt = state.transition(|stack| stack.execute_envelope(envelope))?;
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
    Ok(Json(
        state.transition(|stack| stack.advance(request.milliseconds))?,
    ))
}

async fn reset(State(state): State<ProcessState>) -> std::result::Result<StatusCode, ApiError> {
    state.transition(|stack| {
        stack.reset();
        Ok(())
    })?;
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
    let current_resources = lock(&state)?.resources().clone();
    if restored.resources() != &current_resources {
        return Err(ApiError(ResourceError::new(
            ErrorCode::InvalidValue,
            "the snapshot resources do not match this component process",
        )));
    }
    state.transition(move |stack| {
        *stack = restored;
        Ok(())
    })?;
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
        // Why: remote Stack clients must be able to discover merge before relying on partial
        // mutation persistence; advertising it keeps capability negotiation truthful.
        ResourceKind::Db => &["get", "put", "merge", "delete", "query", "transact"],
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::json;
    use simile_spec::ResourceKind;

    use super::ProcessState;
    use crate::{
        DbCommand, LogicalTime, ResourceAddress, Stack, StackCommand, StackSnapshot, StateStore,
        StateStoreError,
    };

    struct FailingStateStore;

    impl StateStore for FailingStateStore {
        fn load(&self) -> Result<Option<StackSnapshot>, StateStoreError> {
            Ok(None)
        }

        fn save(&self, _snapshot: &StackSnapshot) -> Result<(), StateStoreError> {
            Err(StateStoreError::Database("injected failure".to_owned()))
        }
    }

    #[test]
    fn failed_checkpoint_rolls_back_the_complete_transition()
    -> Result<(), Box<dyn std::error::Error>> {
        let resource = ResourceAddress::new(ResourceKind::Db, "products")?;
        let stack = Stack::builder(LogicalTime::UNIX_EPOCH)
            .resources([resource.clone()])?
            .build()?;
        let before = stack.snapshot()?;
        let state = ProcessState {
            kind: ResourceKind::Db,
            stack: Arc::new(Mutex::new(stack)),
            store: Arc::new(FailingStateStore),
        };

        let result = state.transition(|stack| {
            stack.execute(
                resource,
                Default::default(),
                StackCommand::Db(DbCommand::Put {
                    collection: "products".to_owned(),
                    key: "pr_123".to_owned(),
                    value: json!({"name": "Beat pack"}),
                    condition: Default::default(),
                }),
            )
        });

        assert!(result.is_err());
        let after = state
            .stack
            .lock()
            .map_err(|_| "poisoned stack")?
            .snapshot()?;
        assert_eq!(after, before);
        Ok(())
    }
}
