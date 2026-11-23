#![forbid(unsafe_code)]

//! Protocol-neutral composition of operation execution and Simile Stack.
//!
//! The runtime proves and commits the public operation result. The supervisor
//! then applies synchronous consequences transactionally, checkpoints the
//! complete Stack, and exposes asynchronous work for post-response dispatch.

mod deferred;
mod inspection;
mod profile;
mod profile_name;
mod state;
mod work;

#[cfg(feature = "persistence")]
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use web_time::{SystemTime, UNIX_EPOCH};

use simile_artifact::Artifact;
use simile_runtime::{
    AuthenticatedProfile, ExecutionOptions, IncomingParameters, OperationCall, OperationExecution,
    OperationValueCall, Runtime, authenticate_with_profiles,
};
use simile_spec::ResourceKind;
use simile_stack::{
    CommandMetadata, LogicalTime, MemoryStateStore, QueueCommand, ResourceAddress, ResourceError,
    Stack, StackCommand, StackSnapshot, StateStore, StateStoreError,
};
#[cfg(feature = "persistence")]
use simile_stack::{RedbStateStore, StateIdentity};
use thiserror::Error;

pub use deferred::{DeferredWorkInspection, DependencyReadiness, InspectedDeferredWork};
pub use inspection::{InspectedResource, ServiceStateInspection};
pub use profile::{
    ProfileFieldSummary, ProfileValue, ServiceProfile, ServiceProfileSummary, SetupProfileResult,
    validate_profile_name,
};
pub use simile_spec::ProfileStatus;
use work::{DEAD_LETTERS, OUTBOX, WORK_STATE};
pub use work::{DeadLetter, DeferredCommand, DeferredWork, WorkLease};

pub type Result<T> = std::result::Result<T, Error>;

/// One running service: public operations, declared backends, and durable state.
pub struct Service {
    runtime: Runtime,
    stack: Mutex<Stack>,
    store: Arc<dyn StateStore>,
}

/// Authentication result enriched with the durable profile that supplied it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceAuthentication {
    principal: Option<String>,
    profile: Option<String>,
}

impl ServiceAuthentication {
    #[must_use]
    pub fn principal(&self) -> Option<&str> {
        self.principal.as_deref()
    }

    #[must_use]
    pub fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
    }
}

impl Service {
    /// Build an isolated service with in-memory state.
    pub fn memory(artifact: Artifact) -> Result<Self> {
        Self::with_store(artifact, Arc::new(MemoryStateStore::default()))
    }

    /// Build an isolated service with an explicit clock, including a manual clock
    /// that never reads the host's wall or monotonic time.
    pub fn memory_with_clock(
        artifact: Artifact,
        clock: simile_stack::ClockSnapshot,
    ) -> Result<Self> {
        Self::with_store_and_clock(artifact, Arc::new(MemoryStateStore::default()), clock)
    }

    /// Build a service whose complete Stack survives process restarts.
    #[cfg(feature = "persistence")]
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
        Self::with_store_and_clock(
            artifact,
            store,
            simile_stack::ClockSnapshot {
                mode: simile_stack::ClockMode::Realtime,
                now: now(),
            },
        )
    }

    /// Restore an existing store or initialize it using the supplied clock.
    pub fn with_store_and_clock(
        artifact: Artifact,
        store: Arc<dyn StateStore>,
        clock: simile_stack::ClockSnapshot,
    ) -> Result<Self> {
        let mut resources = artifact
            .resources()
            .map(|resource| ResourceAddress::new(resource.kind(), resource.name()))
            .collect::<simile_stack::Result<Vec<_>>>()?;
        resources.push(ResourceAddress::new(ResourceKind::Queue, OUTBOX)?);
        resources.push(ResourceAddress::new(ResourceKind::Queue, DEAD_LETTERS)?);
        resources.push(ResourceAddress::new(ResourceKind::Db, WORK_STATE)?);
        resources.push(ResourceAddress::new(
            ResourceKind::Db,
            profile::PROFILE_STATE,
        )?);
        let builder = match clock.mode {
            simile_stack::ClockMode::Manual => simile_stack::StackBuilder::manual(clock.now),
            simile_stack::ClockMode::Realtime => Stack::realtime(clock.now),
        };
        let initial = builder.resources(resources)?.build()?;
        let stack = match store.load()? {
            Some(snapshot) => {
                let mut restored = Stack::from_snapshot(snapshot);
                // A refreshed artifact may add backends while existing state remains valid. Add
                // those engines empty and retain retired engines for rollback. The store refreshes
                // artifact provenance during load; a full snapshot write is needed only when the
                // active artifact introduces resources.
                let resources_changed = initial
                    .resources()
                    .iter()
                    .any(|resource| !restored.resources().contains(resource));
                for resource in initial.resources() {
                    restored.ensure_resource(resource.clone());
                }
                if resources_changed {
                    store.save(&restored.snapshot()?)?;
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
        self.execute_for_profile_with_options(call, options, None)
    }

    /// Authenticate against static fixtures and every active service profile.
    pub fn authenticate(
        &self,
        operation: &str,
        incoming: &IncomingParameters,
    ) -> Result<ServiceAuthentication> {
        let profiles = self
            .profile_records()?
            .iter()
            .map(ServiceProfile::credential_profile)
            .collect::<Vec<_>>();
        let authenticated =
            authenticate_with_profiles(self.artifact(), operation, incoming, &profiles)?;
        Ok(service_authentication(authenticated))
    }

    /// Execute one call in the tenant context established by authentication.
    pub fn execute_for_profile_with_options(
        &self,
        call: OperationCall<'_>,
        options: ExecutionOptions<'_>,
        profile: Option<&str>,
    ) -> Result<OperationExecution> {
        let request_received_at = options
            .logical_time_milliseconds
            .map_or_else(|| self.logical_time_milliseconds(), Ok)?;
        let profile = profile
            .map(|name| self.profile(name))
            .transpose()?
            .flatten();
        // Why: POST-based public lookup/page routes carry their selector in JSON rather than in
        // path parameters, so state resolution needs the request value before runtime synthesis.
        let request_body = call
            .content_type
            .filter(|content_type| {
                *content_type == "application/json" || content_type.ends_with("+json")
            })
            .and_then(|_| serde_json::from_slice(call.body).ok());
        let response_request = state::response_request_body(
            self,
            call.operation,
            request_body.as_ref(),
            profile.as_ref().map(|profile| profile.id.as_str()),
        )?;
        let state = state::resolve(
            self,
            call.operation,
            options.parameters,
            request_body.as_ref(),
            profile.as_ref().map(|profile| profile.id.as_str()),
        )?;
        let mutation = if matches!(state, state::StateResolution::None) {
            state::mutation_baseline(
                self,
                call.operation,
                request_body.as_ref(),
                profile.as_ref().map(|profile| profile.id.as_str()),
            )?
        } else {
            state::StateResolution::None
        };
        let response_body = match (&state, &mutation) {
            (state::StateResolution::Found(value), _)
            | (_, state::StateResolution::Found(value)) => Some(value),
            (state::StateResolution::None | state::StateResolution::Missing, _) => {
                options.response_body
            }
        };
        let response_status = match state {
            state::StateResolution::Missing
                if self
                    .artifact()
                    .operation(call.operation)
                    .is_some_and(state::declares_not_found) =>
            {
                Some(404)
            }
            _ => options.response_status,
        };
        let execution = self.runtime.execute_with_options(
            call,
            ExecutionOptions {
                outcome: response_request.outcome.as_deref().or(options.outcome),
                response_body,
                response_status,
                response_request_body: response_request
                    .body
                    .as_ref()
                    .or(options.response_request_body),
                logical_time_milliseconds: Some(request_received_at),
                ..options
            },
        )?;
        self.commit(&execution, options.parameters, profile.as_ref())?;
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
        let request_received_at = options
            .logical_time_milliseconds
            .map_or_else(|| self.logical_time_milliseconds(), Ok)?;
        let response_request = state::response_request_body(self, call.operation, call.body, None)?;
        let state = state::resolve(self, call.operation, options.parameters, call.body, None)?;
        let mutation = if matches!(state, state::StateResolution::None) {
            state::mutation_baseline(self, call.operation, call.body, None)?
        } else {
            state::StateResolution::None
        };
        let response_body = match (&state, &mutation) {
            (state::StateResolution::Found(value), _)
            | (_, state::StateResolution::Found(value)) => Some(value),
            (state::StateResolution::None | state::StateResolution::Missing, _) => {
                options.response_body
            }
        };
        let response_status = match state {
            state::StateResolution::Missing
                if self
                    .artifact()
                    .operation(call.operation)
                    .is_some_and(state::declares_not_found) =>
            {
                Some(404)
            }
            _ => options.response_status,
        };
        let execution = self.runtime.execute_value_with_options(
            call,
            ExecutionOptions {
                outcome: response_request.outcome.as_deref().or(options.outcome),
                response_body,
                response_status,
                response_request_body: response_request
                    .body
                    .as_ref()
                    .or(options.response_request_body),
                logical_time_milliseconds: Some(request_received_at),
                ..options
            },
        )?;
        self.commit(&execution, options.parameters, None)?;
        Ok(execution)
    }

    #[must_use]
    pub const fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// Read the service's authoritative logical time for one transport request.
    pub fn logical_time_milliseconds(&self) -> Result<i64> {
        Ok(lock(&self.stack)?.now()?.as_unix_millis())
    }

    /// Return the compiled contract that defines this service.
    #[must_use]
    pub fn artifact(&self) -> &Artifact {
        self.runtime.artifact()
    }

    pub fn snapshot(&self) -> Result<StackSnapshot> {
        Ok(lock(&self.stack)?.snapshot()?)
    }

    /// Reset every resource and journal entry, then checkpoint the empty Stack.
    pub fn reset_state(&self) -> Result<()> {
        let mut stack = lock(&self.stack)?;
        let before = stack.snapshot()?;
        stack.reset();
        self.checkpoint(&mut stack, before)
    }

    /// Advance a manual service clock and persist the resulting resource changes.
    pub fn advance(&self, milliseconds: u64) -> Result<simile_stack::AdvanceReport> {
        let mut stack = lock(&self.stack)?;
        let before = stack.snapshot()?;
        let report = stack.advance(milliseconds)?;
        self.checkpoint(&mut stack, before)?;
        Ok(report)
    }

    /// Read the service timeline without mutating state or leasing work.
    pub fn clock(&self) -> Result<simile_stack::ClockSnapshot> {
        Ok(self.snapshot()?.clock())
    }

    fn commit(
        &self,
        execution: &OperationExecution,
        parameters: Option<&simile_runtime::ValidatedParameters>,
        profile: Option<&ServiceProfile>,
    ) -> Result<()> {
        let mut stack = lock(&self.stack)?;
        let before = stack.snapshot()?;
        let result = self.apply_commit(&mut stack, execution, parameters, profile);
        if let Err(error) = result {
            *stack = Stack::from_snapshot(before);
            return Err(error);
        }
        self.checkpoint(&mut stack, before)
    }

    fn apply_commit(
        &self,
        stack: &mut Stack,
        execution: &OperationExecution,
        parameters: Option<&simile_runtime::ValidatedParameters>,
        profile: Option<&ServiceProfile>,
    ) -> Result<()> {
        for action in execution.effects().synchronous() {
            let resource = action.resource().as_str().parse::<ResourceAddress>()?;
            let command = StackCommand::from_effect(action.kind(), action.arguments())?;
            // Why: provider-authored effects describe public resources, while a configured
            // profile is an independent local consumer environment whose state must not leak to
            // another credential using the same emulated service.
            let command = state::scope_command(command, profile.map(|profile| profile.id.as_str()));
            stack.execute(
                resource,
                CommandMetadata {
                    operation_id: Some(execution.operation().to_owned()),
                    ..CommandMetadata::default()
                },
                command,
            )?;
        }
        let correlation_id = format!(
            "work-{}",
            stack
                .journal()
                .entries()
                .last()
                .map_or(1, |entry| entry.sequence.saturating_add(1))
        );
        for envelope in work::planned(
            execution,
            &correlation_id,
            profile.map(|profile| profile.name.as_str()),
            profile.and_then(|profile| profile.webhook_target.as_deref()),
        )? {
            let command = QueueCommand::Publish {
                body: work::encode(&envelope.work)?,
                attributes: work::attributes(&envelope.work),
                delay_ms: envelope.delay_milliseconds,
            };
            stack.execute(
                ResourceAddress::new(ResourceKind::Queue, OUTBOX)?,
                CommandMetadata {
                    operation_id: Some(execution.operation().to_owned()),
                    correlation_id: Some(correlation_id.clone()),
                    ..CommandMetadata::default()
                },
                StackCommand::Queue(command),
            )?;
        }
        if let Some((resource, command)) = state::delete_command(
            self.artifact(),
            execution.operation(),
            parameters,
            profile.map(|profile| profile.id.as_str()),
        ) {
            stack.execute(
                resource,
                CommandMetadata {
                    operation_id: Some(execution.operation().to_owned()),
                    ..CommandMetadata::default()
                },
                command,
            )?;
        }
        if let Some(profile) = profile {
            profile::put_profile(stack, profile)?;
        }
        Ok(())
    }

    fn execute_stack(
        &self,
        resource: ResourceAddress,
        metadata: CommandMetadata,
        command: StackCommand,
    ) -> Result<simile_stack::CommandReceipt> {
        let mut stack = lock(&self.stack)?;
        let before = stack.snapshot()?;
        let receipt = match stack.execute(resource, metadata, command) {
            Ok(receipt) => receipt,
            Err(error) => {
                *stack = Stack::from_snapshot(before);
                return Err(error.into());
            }
        };
        self.checkpoint(&mut stack, before)?;
        Ok(receipt)
    }

    pub(crate) fn read_stack(
        &self,
        resource: ResourceAddress,
        command: StackCommand,
    ) -> Result<simile_stack::CommandReceipt> {
        self.execute_stack(resource, CommandMetadata::default(), command)
    }

    fn checkpoint(&self, stack: &mut Stack, before: StackSnapshot) -> Result<()> {
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

fn service_authentication(authenticated: Option<AuthenticatedProfile>) -> ServiceAuthentication {
    let profile = authenticated
        .as_ref()
        .filter(|profile| profile.is_dynamic())
        .map(|profile| profile.name().to_owned());
    let principal = authenticated.map(|profile| profile.principal().to_owned());
    ServiceAuthentication { principal, profile }
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
    #[error("deferred work is invalid: {0}")]
    InvalidWork(String),
    #[error("state-backed response cannot be represented by the operation contract")]
    StateResponseUnavailable,
    #[error("state read returned an unexpected result for {0}")]
    InvalidStateRead(&'static str),
    #[error("state projection for operation `{operation}` failed: {reason}")]
    InvalidStateProjection { operation: String, reason: String },
    #[error(
        "this service does not declare principal setup; add a `principal_setup` block to its configuration"
    )]
    SetupNotDeclared,
    #[error("principal `{principal}` does not declare principal setup")]
    PrincipalSetupNotDeclared { principal: String },
    #[error(
        "this service declares setup for more than one principal ({principals:?}); select one explicitly"
    )]
    PrincipalSetupAmbiguous { principals: Vec<String> },
    #[error("setup operationId `{0}` is missing from the compiled artifact")]
    SetupOperationMissing(String),
    #[error("setup operation `{operation}` has no request body for its collected values")]
    SetupRequestBodyMissing { operation: String },
    #[error("setup requires value `{name}`")]
    MissingSetupValue { name: String },
    #[error("setup does not collect a value named `{name}`")]
    UnknownSetupValue { name: String },
    #[error("setup value `{name}` must be {expected}")]
    InvalidSetupValue {
        name: String,
        expected: &'static str,
    },
    #[error("setup request pointer `{pointer}` cannot be applied to the request body")]
    InvalidSetupPointer { pointer: String },
    #[error("setup response does not contain `{pointer}` required by issued value `{issue}`")]
    SetupResponseValueMissing { issue: String, pointer: String },
    #[error("setup issued credential `{issue}` is not a string")]
    SetupCredentialNotString { issue: String },
    #[error("invalid profile name `{profile}`; use lowercase letters, digits, `.`, `_`, or `-`")]
    InvalidProfileName { profile: String },
    #[error("every generated profile name is already in use; pass --profile with a unique name")]
    ProfileNameSpaceExhausted,
    #[error("profile `{profile}` does not exist")]
    ProfileNotFound { profile: String },
    #[error(
        "profile `{profile}` belongs to principal `{expected}` and cannot be reconfigured as `{actual}`"
    )]
    ProfilePrincipalMismatch {
        profile: String,
        expected: String,
        actual: String,
    },
    #[error("persisted profile state is invalid")]
    InvalidProfileState,
    #[error("persisted profile could not be encoded or decoded: {0}")]
    ProfileEncoding(String),
}
