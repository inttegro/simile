#![forbid(unsafe_code)]

//! Protocol-neutral composition of operation execution and Simile Stack.
//!
//! The runtime proves and commits the public operation result. The supervisor
//! then applies synchronous consequences transactionally, checkpoints the
//! complete Stack, and exposes asynchronous work for post-response dispatch.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use simile_artifact::Artifact;
use simile_runtime::{
    ExecutionOptions, OperationCall, OperationExecution, OperationValueCall, Runtime,
};
use simile_stack::{
    CommandMetadata, LogicalTime, MemoryStateStore, RedbStateStore, ResourceAddress, ResourceError,
    Stack, StackCommand, StackSnapshot, StateIdentity, StateStore, StateStoreError,
};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

/// One running service: public operations, declared resources, and durable state.
pub struct Service {
    runtime: Runtime,
    stack: Mutex<Stack>,
    store: Arc<dyn StateStore>,
}

impl Service {
    /// Build an isolated service with in-memory state.
    pub fn memory(artifact: Artifact) -> Result<Self> {
        Self::with_store(artifact, Arc::new(MemoryStateStore::default()))
    }

    /// Build a service whose complete Stack survives process restarts.
    pub fn durable(artifact: Artifact, path: impl AsRef<Path>) -> Result<Self> {
        let summary = artifact.summary();
        let identity = StateIdentity::new(summary.service, summary.payload_sha256);
        let store = RedbStateStore::open_with_identity(path, identity)?;
        Self::with_store(artifact, Arc::new(store))
    }

    /// Build a service over an injected state store.
    ///
    /// A durable caller normally supplies `RedbStateStore`; tests and one-shot
    /// tools can use `MemoryStateStore` without changing execution semantics.
    pub fn with_store(artifact: Artifact, store: Arc<dyn StateStore>) -> Result<Self> {
        let resources = artifact
            .resources()
            .map(|resource| ResourceAddress::new(resource.kind(), resource.name()))
            .collect::<simile_stack::Result<Vec<_>>>()?;
        let initial = Stack::realtime(now()).resources(resources)?.build()?;
        let stack = match store.load()? {
            Some(snapshot) => {
                let restored = Stack::from_snapshot(snapshot);
                if restored.resources() != initial.resources() {
                    return Err(Error::ResourceSetChanged);
                }
                restored
            }
            None => {
                store.save(&initial.snapshot()?)?;
                initial
            }
        };
        Ok(Self {
            runtime: Runtime::from_artifact(artifact),
            stack: Mutex::new(stack),
            store,
        })
    }

    pub fn execute(&self, call: OperationCall<'_>) -> Result<OperationExecution> {
        self.execute_with_options(call, ExecutionOptions::default())
    }

    pub fn execute_with_options(
        &self,
        call: OperationCall<'_>,
        options: ExecutionOptions<'_>,
    ) -> Result<OperationExecution> {
        let execution = self.runtime.execute_with_options(call, options)?;
        self.apply_synchronous(&execution)?;
        Ok(execution)
    }

    pub fn execute_value(&self, call: OperationValueCall<'_>) -> Result<OperationExecution> {
        self.execute_value_with_options(call, ExecutionOptions::default())
    }

    pub fn execute_value_with_options(
        &self,
        call: OperationValueCall<'_>,
        options: ExecutionOptions<'_>,
    ) -> Result<OperationExecution> {
        let execution = self.runtime.execute_value_with_options(call, options)?;
        self.apply_synchronous(&execution)?;
        Ok(execution)
    }

    /// Dispatch work that must not contribute to immediate response latency.
    ///
    /// Transport adapters schedule this outside the response-critical path.
    /// Every named async unit checkpoints atomically, so a failure cannot leave
    /// half of that unit visible in durable state.
    pub fn dispatch_asynchronous(&self, execution: &OperationExecution) -> Result<()> {
        for operation in execution.effects().asynchronous() {
            self.transition(execution.operation(), operation.actions())?;
        }
        Ok(())
    }

    #[must_use]
    pub const fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// Return the compiled contract that defines this service.
    #[must_use]
    pub fn artifact(&self) -> &Artifact {
        self.runtime.artifact()
    }

    pub fn snapshot(&self) -> Result<StackSnapshot> {
        Ok(lock(&self.stack)?.snapshot()?)
    }

    fn apply_synchronous(&self, execution: &OperationExecution) -> Result<()> {
        if execution.effects().synchronous().is_empty() {
            return Ok(());
        }
        self.transition(execution.operation(), execution.effects().synchronous())
    }

    fn transition(&self, operation: &str, actions: &[simile_runtime::PlannedAction]) -> Result<()> {
        let mut stack = lock(&self.stack)?;
        let before = stack.snapshot()?;
        for action in actions {
            let resource = action.resource().as_str().parse::<ResourceAddress>()?;
            let command = StackCommand::from_effect(action.kind(), action.arguments())?;
            if let Err(error) = stack.execute(
                resource,
                CommandMetadata {
                    operation_id: Some(operation.to_owned()),
                    ..CommandMetadata::default()
                },
                command,
            ) {
                *stack = Stack::from_snapshot(before.clone());
                return Err(error.into());
            }
        }
        let after = match stack.snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                *stack = Stack::from_snapshot(before.clone());
                return Err(error.into());
            }
        };
        if let Err(error) = self.store.save(&after) {
            *stack = Stack::from_snapshot(before);
            return Err(error.into());
        }
        Ok(())
    }
}

fn now() -> LogicalTime {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default();
    LogicalTime::from_unix_millis(milliseconds)
}

fn lock(stack: &Mutex<Stack>) -> Result<MutexGuard<'_, Stack>> {
    stack.lock().map_err(|_| Error::StateUnavailable)
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Runtime(#[from] simile_runtime::Error),
    #[error(transparent)]
    Resource(#[from] ResourceError),
    #[error(transparent)]
    Store(#[from] StateStoreError),
    #[error("service state is unavailable")]
    StateUnavailable,
    #[error("persisted resources do not match the compiled service")]
    ResourceSetChanged,
}
