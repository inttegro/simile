use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use simile_artifact::{Artifact, ArtifactOutcome};
use simile_spec::{OutcomeProvenance, ResourceRef, ResponseStatus};

use crate::{
    Error, IncomingBody, IncomingValue, ResponseSynthesisRequest, Result, SynthesizedResponse,
    ValidatedBody, synthesize_response, validate_request_body, validate_request_value,
};

/// Executes compiled service operations without depending on a transport framework.
#[derive(Debug)]
pub struct Runtime {
    artifact: Arc<Artifact>,
    replays: Mutex<HashMap<ReplayKey, ReplayRecord>>,
}

/// One operation invocation selected by a transport or another runtime client.
#[derive(Clone, Copy, Debug)]
pub struct OperationCall<'a> {
    pub operation: &'a str,
    pub content_type: Option<&'a str>,
    pub body: &'a [u8],
}

/// One operation invocation whose request body has already been decoded by its adapter.
#[derive(Clone, Copy, Debug)]
pub struct OperationValueCall<'a> {
    pub operation: &'a str,
    pub content_type: Option<&'a str>,
    pub body: Option<&'a Value>,
}

/// An identity that makes a repeated invocation replay its first completed outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayIdentity<'a> {
    /// A caller-supplied idempotency key for a mutation.
    IdempotencyKey(&'a str),
    /// The stable identity of a resource addressed by a GET request.
    Resource(&'a str),
}

/// Optional controls for scenarios that need a specific declared response.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExecutionOptions<'a> {
    pub outcome: Option<&'a str>,
    pub scenario: Option<&'a str>,
    pub response_status: Option<u16>,
    pub response_content_type: Option<&'a str>,
    pub replay: Option<ReplayIdentity<'a>>,
}

/// The validated request, immediate response, and declared follow-up work for an operation.
#[derive(Clone, Debug, PartialEq)]
pub struct OperationOutcome {
    operation: String,
    request_body: Option<ValidatedBody>,
    response: SynthesizedResponse,
    selected_outcome: Option<String>,
    provenance: Option<OutcomeProvenance>,
    async_operations: Vec<PlannedAsyncOperation>,
}

/// One asynchronous operation declared by the selected behavior.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedAsyncOperation {
    name: String,
    resources: Vec<ResourceRef>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum ReplayKey {
    Idempotency { operation: String, key: String },
    Resource { operation: String, id: String },
}

#[derive(Clone, Debug)]
struct ReplayRecord {
    request_body: Option<ValidatedBody>,
    outcome: OperationOutcome,
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
            artifact,
            replays: Mutex::new(HashMap::new()),
        }
    }

    /// Execute an operation using the response selected by its compiled contract and behavior.
    pub fn execute(&self, call: OperationCall<'_>) -> Result<OperationOutcome> {
        self.execute_with_options(call, ExecutionOptions::default())
    }

    /// Execute an operation with explicit response controls for a test scenario.
    pub fn execute_with_options(
        &self,
        call: OperationCall<'_>,
        options: ExecutionOptions<'_>,
    ) -> Result<OperationOutcome> {
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
    pub fn execute_value(&self, call: OperationValueCall<'_>) -> Result<OperationOutcome> {
        self.execute_value_with_options(call, ExecutionOptions::default())
    }

    /// Execute a decoded operation value with explicit response controls.
    pub fn execute_value_with_options(
        &self,
        call: OperationValueCall<'_>,
        options: ExecutionOptions<'_>,
    ) -> Result<OperationOutcome> {
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

    /// Synthesize the first service response declared for one of `statuses`.
    ///
    /// Exact responses win over class responses such as `4XX`, and class
    /// responses win over `default`. Candidate order is retained within each
    /// tier. This lets a transport ask the emulated service to own a protocol
    /// failure without mistaking an unrelated success response for an error.
    pub fn synthesize_first_declared_response(
        &self,
        operation_key: &str,
        statuses: &[u16],
    ) -> Result<Option<SynthesizedResponse>> {
        let operation =
            self.artifact
                .operation(operation_key)
                .ok_or_else(|| Error::UnknownOperation {
                    operation: operation_key.to_owned(),
                })?;
        let responses = operation.responses().collect::<Vec<_>>();
        let status = statuses
            .iter()
            .copied()
            .find(|status| {
                responses
                    .iter()
                    .any(|response| response.status() == &ResponseStatus::Exact(*status))
            })
            .or_else(|| {
                statuses.iter().copied().find(|status| {
                    responses.iter().any(|response| {
                        matches!(response.status(), ResponseStatus::Range(_))
                            && response.status().matches(*status)
                    })
                })
            })
            .or_else(|| {
                responses
                    .iter()
                    .any(|response| response.status() == &ResponseStatus::Default)
                    .then(|| statuses.first().copied())
                    .flatten()
            });
        let Some(status) = status else {
            return Ok(None);
        };
        synthesize_response(
            &self.artifact,
            ResponseSynthesisRequest {
                operation: operation_key,
                status: Some(status),
                content_type: None,
                request_body: None,
                body: None,
            },
        )
        .map(Some)
    }

    /// Render the service-declared response for a transport/runtime failure.
    pub fn synthesize_protocol_error(
        &self,
        operation_key: &str,
        cause: &str,
    ) -> Result<Option<SynthesizedResponse>> {
        let Some(mapping) = self.artifact.protocol_error(cause) else {
            return Ok(None);
        };
        let operation =
            self.artifact
                .operation(operation_key)
                .ok_or_else(|| Error::UnknownOperation {
                    operation: operation_key.to_owned(),
                })?;
        let status = mapping.statuses().iter().copied().find(|status| {
            operation
                .responses()
                .any(|response| response.status().matches(*status))
        });
        let Some(status) = status else {
            return Ok(None);
        };
        synthesize_response(
            &self.artifact,
            ResponseSynthesisRequest {
                operation: operation_key,
                status: Some(status),
                content_type: mapping.content_type(),
                request_body: None,
                body: mapping.body(),
            },
        )
        .map(Some)
    }

    fn complete(
        &self,
        operation_key: &str,
        request_body: Option<ValidatedBody>,
        options: ExecutionOptions<'_>,
    ) -> Result<OperationOutcome> {
        let Some(identity) = options.replay else {
            return self.produce_outcome(operation_key, request_body, options);
        };
        let key = replay_key(operation_key, identity)?;
        let mut replays = self
            .replays
            .lock()
            .map_err(|_| Error::ReplayStateUnavailable)?;
        if let Some(record) = replays.get(&key) {
            if matches!(key, ReplayKey::Idempotency { .. }) && record.request_body != request_body {
                return Err(Error::IdempotencyConflict {
                    operation: operation_key.to_owned(),
                });
            }
            return Ok(record.outcome.clone());
        }
        let outcome = self.produce_outcome(operation_key, request_body.clone(), options)?;
        replays.insert(
            key,
            ReplayRecord {
                request_body,
                outcome: outcome.clone(),
            },
        );
        Ok(outcome)
    }

    fn produce_outcome(
        &self,
        operation_key: &str,
        request_body: Option<ValidatedBody>,
        options: ExecutionOptions<'_>,
    ) -> Result<OperationOutcome> {
        let operation =
            self.artifact
                .operation(operation_key)
                .ok_or_else(|| Error::UnknownOperation {
                    operation: operation_key.to_owned(),
                })?;
        let selected = self.select_outcome(operation_key, request_body.as_ref(), options)?;
        let response_status = selected
            .as_ref()
            .map_or(options.response_status, |outcome| Some(outcome.status()));
        let response_content_type = selected
            .as_ref()
            .map_or(options.response_content_type, |outcome| {
                outcome.content_type().or(options.response_content_type)
            });
        let response = synthesize_response(
            &self.artifact,
            ResponseSynthesisRequest {
                operation: operation_key,
                status: response_status,
                content_type: response_content_type,
                request_body: request_body.as_ref(),
                body: selected.as_ref().and_then(ArtifactOutcome::body),
            },
        )?;
        let async_operations = operation
            .triggers()
            .map(|trigger| PlannedAsyncOperation {
                name: trigger.name().to_owned(),
                resources: trigger
                    .resources()
                    .map(|resource| resource.reference())
                    .collect(),
            })
            .collect();

        Ok(OperationOutcome {
            operation: operation_key.to_owned(),
            request_body,
            response,
            selected_outcome: selected.as_ref().map(|outcome| outcome.name().to_owned()),
            provenance: selected.as_ref().map(ArtifactOutcome::provenance),
            async_operations,
        })
    }

    fn select_outcome(
        &self,
        operation_key: &str,
        request_body: Option<&ValidatedBody>,
        options: ExecutionOptions<'_>,
    ) -> Result<Option<ArtifactOutcome>> {
        let operation =
            self.artifact
                .operation(operation_key)
                .ok_or_else(|| Error::UnknownOperation {
                    operation: operation_key.to_owned(),
                })?;
        let named = if let Some(outcome) = options.outcome {
            Some(outcome)
        } else if let Some(scenario) = options.scenario {
            if !self.artifact.has_scenario(scenario) {
                return Err(Error::UnknownScenario {
                    scenario: scenario.to_owned(),
                });
            }
            self.artifact.scenario_outcome(scenario, operation_key)
        } else {
            None
        };
        if let Some(name) = named {
            return operation
                .outcomes()
                .find(|outcome| outcome.name() == name)
                .map(Some)
                .ok_or_else(|| Error::UnknownOutcome {
                    operation: operation_key.to_owned(),
                    outcome: name.to_owned(),
                });
        }
        let Some(body) = request_body.map(ValidatedBody::value) else {
            return Ok(None);
        };
        Ok(operation.outcomes().find(|outcome| {
            !outcome.conditions().is_empty()
                && outcome
                    .conditions()
                    .iter()
                    .all(|condition| body.pointer(condition.path()) == Some(condition.equals()))
        }))
    }
}

fn replay_key(operation: &str, identity: ReplayIdentity<'_>) -> Result<ReplayKey> {
    match identity {
        ReplayIdentity::IdempotencyKey(key) => {
            let key = key.trim();
            if key.is_empty() {
                return Err(Error::EmptyReplayIdentity);
            }
            Ok(ReplayKey::Idempotency {
                operation: operation.to_owned(),
                key: key.to_owned(),
            })
        }
        ReplayIdentity::Resource(id) => {
            if !operation.starts_with("GET ") {
                return Err(Error::ResourceReplayRequiresGet {
                    operation: operation.to_owned(),
                });
            }
            let id = id.trim();
            if id.is_empty() {
                return Err(Error::EmptyReplayIdentity);
            }
            Ok(ReplayKey::Resource {
                operation: operation.to_owned(),
                id: id.to_owned(),
            })
        }
    }
}

impl OperationOutcome {
    #[must_use]
    pub fn operation(&self) -> &str {
        &self.operation
    }

    #[must_use]
    pub const fn request_body(&self) -> Option<&ValidatedBody> {
        self.request_body.as_ref()
    }

    #[must_use]
    pub const fn response(&self) -> &SynthesizedResponse {
        &self.response
    }

    #[must_use]
    pub fn selected_outcome(&self) -> Option<&str> {
        self.selected_outcome.as_deref()
    }

    #[must_use]
    pub const fn provenance(&self) -> Option<OutcomeProvenance> {
        self.provenance
    }

    #[must_use]
    pub fn async_operations(&self) -> &[PlannedAsyncOperation] {
        &self.async_operations
    }

    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        String,
        Option<ValidatedBody>,
        SynthesizedResponse,
        Vec<PlannedAsyncOperation>,
    ) {
        (
            self.operation,
            self.request_body,
            self.response,
            self.async_operations,
        )
    }
}

impl PlannedAsyncOperation {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn resources(&self) -> &[ResourceRef] {
        &self.resources
    }
}
