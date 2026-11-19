use std::sync::{Arc, Mutex};

use simile_artifact::Artifact;

use crate::fault::FaultController;
use crate::outcome;
use crate::replay::ReplayStore;
use crate::{
    Error, ExecutionOptions, IncomingBody, IncomingValue, OperationCall, OperationExecution,
    OperationOutcome, OperationValueCall, Result, ValidatedBody, validate_request_body,
    validate_request_value,
};

/// Executes compiled service operations without depending on a transport framework.
#[derive(Debug)]
pub struct Runtime {
    artifact: Arc<Artifact>,
    replays: ReplayStore,
    faults: Mutex<FaultController>,
}

impl Runtime {
    #[must_use]
    pub fn new(artifact: &Artifact) -> Self {
        Self::from_shared(Arc::new(artifact.clone()))
    }

    #[must_use]
    pub fn from_artifact(artifact: Artifact) -> Self {
        Self::from_shared(Arc::new(artifact))
    }

    #[must_use]
    pub fn from_shared(artifact: Arc<Artifact>) -> Self {
        Self {
            faults: Mutex::new(FaultController::new(&artifact)),
            artifact,
            replays: ReplayStore::default(),
        }
    }

    /// Execute an operation using the response selected by its compiled contract and behavior.
    pub fn execute(&self, call: OperationCall<'_>) -> Result<OperationExecution> {
        self.execute_with_options(call, ExecutionOptions::default())
    }

    /// Execute an operation with explicit response controls for a test scenario.
    pub fn execute_with_options(
        &self,
        call: OperationCall<'_>,
        options: ExecutionOptions<'_>,
    ) -> Result<OperationExecution> {
        let request_body = validate_request_body(
            &self.artifact,
            IncomingBody {
                operation: call.operation,
                content_type: call.content_type,
                bytes: call.body,
            },
        )?;
        self.complete(call.operation, request_body, options)
    }

    /// Execute an operation from a transport-neutral value decoded by an adapter.
    pub fn execute_value(&self, call: OperationValueCall<'_>) -> Result<OperationExecution> {
        self.execute_value_with_options(call, ExecutionOptions::default())
    }

    /// Execute a decoded operation value with explicit response controls.
    pub fn execute_value_with_options(
        &self,
        call: OperationValueCall<'_>,
        options: ExecutionOptions<'_>,
    ) -> Result<OperationExecution> {
        let operation =
            self.artifact
                .operation(call.operation)
                .ok_or_else(|| Error::UnknownOperation {
                    operation: call.operation.to_owned(),
                })?;
        let request_body = match call.body {
            Some(value) => Some(validate_request_value(
                &self.artifact,
                IncomingValue {
                    operation: call.operation,
                    content_type: call.content_type,
                    value,
                },
            )?),
            None if operation
                .request_body()
                .is_some_and(|body| body.is_required()) =>
            {
                return Err(Error::BodyRequired {
                    operation: call.operation.to_owned(),
                });
            }
            None => None,
        };
        self.complete(call.operation, request_body, options)
    }

    fn complete(
        &self,
        operation_key: &str,
        request_body: Option<ValidatedBody>,
        options: ExecutionOptions<'_>,
    ) -> Result<OperationExecution> {
        let Some(identity) = options.replay else {
            let outcome = outcome::produce(&self.artifact, operation_key, request_body, options)?;
            let effects = crate::effect::plan(&self.artifact, operation_key, &outcome)?;
            return self.finish_attempt(operation_key, outcome, effects, options);
        };

        let replay = self
            .replays
            .resolve(operation_key, identity, request_body.clone(), || {
                let outcome =
                    outcome::produce(&self.artifact, operation_key, request_body, options)?;
                let effects = crate::effect::plan(&self.artifact, operation_key, &outcome)?;
                Ok((outcome, effects))
            })?;
        if replay.replayed {
            return Ok(OperationExecution::normal(replay.outcome));
        }
        self.finish_attempt(operation_key, replay.outcome, replay.effects, options)
    }

    fn finish_attempt(
        &self,
        operation_key: &str,
        outcome: OperationOutcome,
        effects: crate::EffectPlan,
        options: ExecutionOptions<'_>,
    ) -> Result<OperationExecution> {
        let fault = self
            .faults
            .lock()
            .map_err(|_| Error::FaultStateUnavailable)?
            .activate(
                &self.artifact,
                operation_key,
                options.scenario,
                options.fault_profile,
            )?;
        Ok(OperationExecution::with_fault(outcome, effects, fault))
    }

    /// Return the seed governing one stochastic profile in this runtime.
    ///
    /// Profiles without a configured seed receive one at runtime construction;
    /// exposing it makes an otherwise random session exactly reproducible.
    pub fn fault_profile_seed(&self, profile: &str) -> Result<u64> {
        self.faults
            .lock()
            .map_err(|_| Error::FaultStateUnavailable)?
            .seed(profile)
            .ok_or_else(|| Error::UnknownFaultProfile {
                profile: profile.to_owned(),
            })
    }

    /// Return the immutable compiled contract used by this runtime.
    ///
    /// Transport and supervision layers use this for routing and capability
    /// discovery. Operation execution remains owned by `Runtime`.
    #[must_use]
    pub fn artifact(&self) -> &Artifact {
        &self.artifact
    }
}
