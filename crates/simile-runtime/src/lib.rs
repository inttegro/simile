#![forbid(unsafe_code)]

mod effect;
mod execution;
mod fault;
mod operation;
mod outcome;
mod parameter;
mod protocol;
mod replay;
mod response;
mod webhook;

use serde_json::Value;
use simile_artifact::Artifact;
use simile_schema::{SynthesisError, ValidationIssue};
use thiserror::Error;

pub use effect::{EffectPlan, PlannedAction, PlannedAsyncOperation, PlannedWebhook};
pub use execution::{
    ExecutionOptions, OperationCall, OperationExecution, OperationOutcome, OperationValueCall,
    ReplayIdentity,
};
pub use fault::{FaultActivation, FaultSource, TransportPlan};
pub use operation::Runtime;
pub use parameter::{IncomingParameters, ParameterIssue, ValidatedParameters, validate_parameters};
pub use response::{
    ResponseSoundness, ResponseSynthesisRequest, SynthesizedResponse, synthesize_response,
    synthesize_response_with_seed,
};
pub use webhook::{
    WebhookDeliveryPair, WebhookOrder, WebhookOrderingDecision, WebhookOrderingReason,
    decide_webhook_order,
};

pub type Result<T> = std::result::Result<T, Error>;

/// The request-body portion of an incoming HTTP request.
#[derive(Clone, Copy, Debug)]
pub struct IncomingBody<'a> {
    pub operation: &'a str,
    pub content_type: Option<&'a str>,
    pub bytes: &'a [u8],
}

/// Selects an operation and, optionally, one of its declared request media types.
#[derive(Clone, Copy, Debug)]
pub struct SynthesisRequest<'a> {
    pub operation: &'a str,
    pub content_type: Option<&'a str>,
}

/// A normalized request value selected against one declared OpenAPI media type.
#[derive(Clone, Copy, Debug)]
pub struct IncomingValue<'a> {
    pub operation: &'a str,
    pub content_type: Option<&'a str>,
    pub value: &'a Value,
}

/// A normalized request value synthesized from a compiled OpenAPI contract.
#[derive(Clone, Debug, PartialEq)]
pub struct SynthesizedValue {
    media_type: String,
    value: Value,
}

impl SynthesizedValue {
    #[must_use]
    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    #[must_use]
    pub const fn value(&self) -> &Value {
        &self.value
    }

    #[must_use]
    pub fn into_parts(self) -> (String, Value) {
        (self.media_type, self.value)
    }
}

/// A request body synthesized from a compiled OpenAPI contract.
#[derive(Clone, Debug, PartialEq)]
pub struct SynthesizedBody {
    media_type: String,
    bytes: Vec<u8>,
    value: Value,
}

impl SynthesizedBody {
    #[must_use]
    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub const fn value(&self) -> &Value {
        &self.value
    }

    #[must_use]
    pub fn into_parts(self) -> (String, Vec<u8>, Value) {
        (self.media_type, self.bytes, self.value)
    }
}

/// A body proven sound against its compiled OpenAPI schema and cross-field relationships.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedBody {
    operation: String,
    media_type: String,
    value: Value,
}

impl ValidatedBody {
    #[must_use]
    pub fn operation(&self) -> &str {
        &self.operation
    }

    #[must_use]
    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    #[must_use]
    pub const fn value(&self) -> &Value {
        &self.value
    }

    #[must_use]
    pub fn into_value(self) -> Value {
        self.value
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("operation `{operation}` is not present in the compiled service")]
    UnknownOperation { operation: String },

    #[error("operation `{operation}` has no named outcome `{outcome}`")]
    UnknownOutcome { operation: String, outcome: String },

    #[error("scenario `{scenario}` is not present in the compiled service")]
    UnknownScenario { scenario: String },

    #[error("fault profile `{profile}` is not present in the compiled service")]
    UnknownFaultProfile { profile: String },

    #[error("fault `{fault}` is not present in the compiled service")]
    UnknownFault { fault: String },

    #[error("runtime fault state is unavailable")]
    FaultStateUnavailable,

    #[error("effect `{effect}` references missing async operation `{operation}`")]
    EffectAsyncMissing { effect: String, operation: String },

    #[error("effect references missing webhook `{webhook}`")]
    EffectWebhookMissing { webhook: String },

    #[error("webhook `{webhook}` has no selectable request content type")]
    EffectWebhookMediaTypeMissing { webhook: String },

    #[error("webhook target must resolve to a non-empty string")]
    InvalidWebhookTarget,

    #[error("OpenAPI callback expression `{expression}` is not supported")]
    UnsupportedCallbackExpression { expression: String },

    #[error("OpenAPI callback expression `{expression}` did not resolve to a URL")]
    CallbackTargetMissing { expression: String },

    #[error("webhook `{webhook}` body failed schema validation with {} issue(s)", .issues.len())]
    InvalidWebhookBody {
        webhook: String,
        issues: Vec<ValidationIssue>,
    },

    #[error(
        "effect argument `{argument}` cannot project {document} value at JSON pointer `{pointer}`"
    )]
    EffectProjectionMissing {
        document: &'static str,
        pointer: String,
        argument: String,
    },

    #[error("operation `{operation}` does not declare a request body")]
    BodyNotDeclared { operation: String },

    #[error("operation `{operation}` requires a request body")]
    BodyRequired { operation: String },

    #[error("a Content-Type header is required when `{operation}` has a request body")]
    MissingContentType { operation: String },

    #[error(
        "Content-Type `{content_type}` is not declared for `{operation}`; expected one of {expected:?}"
    )]
    UnsupportedMediaType {
        operation: String,
        content_type: String,
        expected: Vec<String>,
    },

    #[error("Content-Type `{content_type}` does not yet have a request decoder")]
    UnsupportedEncoding { content_type: String },

    #[error("operation `{operation}` has no declared media type to synthesize")]
    NoSynthesisMediaType {
        operation: String,
        expected: Vec<String>,
    },

    #[error("operation `{operation}` does not declare a response matching HTTP {status}")]
    ResponseNotDeclared { operation: String, status: u16 },

    #[error("operation `{operation}` declares no response body for HTTP {status}")]
    ResponseBodyNotDeclared { operation: String, status: u16 },

    #[error("a replay identity cannot be empty")]
    EmptyReplayIdentity,

    #[error("resource replay is only valid for GET operations, not `{operation}`")]
    ResourceReplayRequiresGet { operation: String },

    #[error("idempotency key was already used with a different request for `{operation}`")]
    IdempotencyConflict { operation: String },

    #[error("runtime replay state is unavailable")]
    ReplayStateUnavailable,

    #[error("operation `{operation}` has no concrete response status to synthesize")]
    NoResponseStatus { operation: String },

    #[error(
        "validated request context belongs to `{request_operation}`, not `{response_operation}`"
    )]
    RequestContextMismatch {
        request_operation: String,
        response_operation: String,
    },

    #[error("operation `{operation}` has no declared JSON response media type to synthesize")]
    NoResponseMediaType {
        operation: String,
        expected: Vec<String>,
    },

    #[error(
        "response Content-Type `{content_type}` is not declared for `{operation}`; expected one of {expected:?}"
    )]
    UnsupportedResponseMediaType {
        operation: String,
        content_type: String,
        expected: Vec<String>,
    },

    #[error("response Content-Type `{content_type}` does not yet have an encoder")]
    UnsupportedResponseEncoding { content_type: String },

    #[error("could not synthesize a request body: {source}")]
    Synthesis {
        #[source]
        source: SynthesisError,
    },

    #[error("could not synthesize a response body: {source}")]
    ResponseSynthesis {
        #[source]
        source: SynthesisError,
    },

    #[error("could not encode a synthesized JSON request body: {message}")]
    EncodeJson { message: String },

    #[error("could not encode a synthesized JSON response body: {message}")]
    EncodeResponseJson { message: String },

    #[error("request body is not valid JSON at line {line}, column {column}: {message}")]
    MalformedJson {
        line: usize,
        column: usize,
        message: String,
    },

    #[error("request body failed schema validation with {} issue(s)", .issues.len())]
    InvalidBody { issues: Vec<ValidationIssue> },

    #[error("request parameters failed contract validation with {} issue(s)", .issues.len())]
    InvalidParameters { issues: Vec<ParameterIssue> },

    #[error("synthesized response body failed schema validation with {} issue(s)", .issues.len())]
    InvalidResponseBody { issues: Vec<ValidationIssue> },
}

impl Error {
    #[must_use]
    pub fn issues(&self) -> &[ValidationIssue] {
        match self {
            Self::InvalidBody { issues }
            | Self::InvalidResponseBody { issues }
            | Self::InvalidWebhookBody { issues, .. } => issues,
            _ => &[],
        }
    }
}

/// Decode and validate an incoming request body using the compiled operation contract.
pub fn validate_request_body(
    artifact: &Artifact,
    request: IncomingBody<'_>,
) -> Result<Option<ValidatedBody>> {
    let operation =
        artifact
            .operation(request.operation)
            .ok_or_else(|| Error::UnknownOperation {
                operation: request.operation.to_owned(),
            })?;
    let plan = operation.request_body();

    if request.bytes.is_empty() {
        if plan.is_some_and(|plan| plan.is_required()) {
            return Err(Error::BodyRequired {
                operation: request.operation.to_owned(),
            });
        }
        return Ok(None);
    }
    let plan = plan.ok_or_else(|| Error::BodyNotDeclared {
        operation: request.operation.to_owned(),
    })?;
    let content_type = request
        .content_type
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Error::MissingContentType {
            operation: request.operation.to_owned(),
        })?;
    let selected = plan
        .select(content_type)
        .ok_or_else(|| Error::UnsupportedMediaType {
            operation: request.operation.to_owned(),
            content_type: content_type.to_owned(),
            expected: plan.media_types().map(str::to_owned).collect(),
        })?;
    if !is_json(selected.requested()) {
        return Err(Error::UnsupportedEncoding {
            content_type: selected.requested().to_owned(),
        });
    }
    let value = serde_json::from_slice(request.bytes).map_err(|error| Error::MalformedJson {
        line: error.line(),
        column: error.column(),
        message: error.to_string(),
    })?;
    let issues = selected.validate(&value);
    if !issues.is_empty() {
        return Err(Error::InvalidBody { issues });
    }
    Ok(Some(ValidatedBody {
        operation: request.operation.to_owned(),
        media_type: selected.requested().to_owned(),
        value,
    }))
}

/// Validate a transport-neutral request value against one declared OpenAPI media type.
pub fn validate_request_value(
    artifact: &Artifact,
    request: IncomingValue<'_>,
) -> Result<ValidatedBody> {
    let operation =
        artifact
            .operation(request.operation)
            .ok_or_else(|| Error::UnknownOperation {
                operation: request.operation.to_owned(),
            })?;
    let plan = operation
        .request_body()
        .ok_or_else(|| Error::BodyNotDeclared {
            operation: request.operation.to_owned(),
        })?;
    let content_type = select_request_media_type(&plan, request.content_type).ok_or_else(|| {
        Error::NoSynthesisMediaType {
            operation: request.operation.to_owned(),
            expected: plan.media_types().map(str::to_owned).collect(),
        }
    })?;
    let selected = plan
        .select(&content_type)
        .ok_or_else(|| Error::UnsupportedMediaType {
            operation: request.operation.to_owned(),
            content_type: content_type.clone(),
            expected: plan.media_types().map(str::to_owned).collect(),
        })?;
    let issues = selected.validate(request.value);
    if !issues.is_empty() {
        return Err(Error::InvalidBody { issues });
    }
    Ok(ValidatedBody {
        operation: request.operation.to_owned(),
        media_type: selected.requested().to_owned(),
        value: request.value.clone(),
    })
}

/// Synthesize a fresh transport-neutral request value from the compiled schema.
pub fn synthesize_request_value(
    artifact: &Artifact,
    request: SynthesisRequest<'_>,
) -> Result<SynthesizedValue> {
    synthesize_request_value_with_seed(artifact, request, rand::random())
}

/// Synthesize a reproducible transport-neutral request value from the compiled schema.
pub fn synthesize_request_value_with_seed(
    artifact: &Artifact,
    request: SynthesisRequest<'_>,
    seed: u64,
) -> Result<SynthesizedValue> {
    let operation =
        artifact
            .operation(request.operation)
            .ok_or_else(|| Error::UnknownOperation {
                operation: request.operation.to_owned(),
            })?;
    let plan = operation
        .request_body()
        .ok_or_else(|| Error::BodyNotDeclared {
            operation: request.operation.to_owned(),
        })?;
    let content_type = select_request_media_type(&plan, request.content_type).ok_or_else(|| {
        Error::NoSynthesisMediaType {
            operation: request.operation.to_owned(),
            expected: plan.media_types().map(str::to_owned).collect(),
        }
    })?;
    let selected = plan
        .select(&content_type)
        .ok_or_else(|| Error::UnsupportedMediaType {
            operation: request.operation.to_owned(),
            content_type: content_type.clone(),
            expected: plan.media_types().map(str::to_owned).collect(),
        })?;
    let value = selected
        .synthesize_fuzz(seed)
        .map_err(|source| Error::Synthesis { source })?;
    let issues = selected.validate(&value);
    if !issues.is_empty() {
        return Err(Error::InvalidBody { issues });
    }
    Ok(SynthesizedValue {
        media_type: selected.requested().to_owned(),
        value,
    })
}

/// Synthesize a fresh JSON body and verify it against the same compiled request schema.
pub fn synthesize_request_body(
    artifact: &Artifact,
    request: SynthesisRequest<'_>,
) -> Result<SynthesizedBody> {
    synthesize_request_body_with_seed(artifact, request, rand::random())
}

/// Synthesize a reproducible JSON body and verify it against the compiled request schema.
pub fn synthesize_request_body_with_seed(
    artifact: &Artifact,
    request: SynthesisRequest<'_>,
    seed: u64,
) -> Result<SynthesizedBody> {
    let operation =
        artifact
            .operation(request.operation)
            .ok_or_else(|| Error::UnknownOperation {
                operation: request.operation.to_owned(),
            })?;
    let plan = operation
        .request_body()
        .ok_or_else(|| Error::BodyNotDeclared {
            operation: request.operation.to_owned(),
        })?;
    let content_type = request
        .content_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| preferred_json_media_type(&plan));
    let content_type = content_type.ok_or_else(|| Error::NoSynthesisMediaType {
        operation: request.operation.to_owned(),
        expected: plan.media_types().map(str::to_owned).collect(),
    })?;
    let selected = plan
        .select(&content_type)
        .ok_or_else(|| Error::UnsupportedMediaType {
            operation: request.operation.to_owned(),
            content_type: content_type.clone(),
            expected: plan.media_types().map(str::to_owned).collect(),
        })?;
    if !is_json(selected.requested()) {
        return Err(Error::UnsupportedEncoding {
            content_type: selected.requested().to_owned(),
        });
    }
    let value = selected
        .synthesize_fuzz(seed)
        .map_err(|source| Error::Synthesis { source })?;
    let issues = selected.validate(&value);
    if !issues.is_empty() {
        return Err(Error::InvalidBody { issues });
    }
    let bytes = serde_json::to_vec(&value).map_err(|error| Error::EncodeJson {
        message: error.to_string(),
    })?;
    Ok(SynthesizedBody {
        media_type: selected.requested().to_owned(),
        bytes,
        value,
    })
}

fn preferred_json_media_type(plan: &simile_schema::BoundRequestBody<'_>) -> Option<String> {
    plan.media_types()
        .find(|media_type| is_json(media_type))
        .map(str::to_owned)
        .or_else(|| {
            plan.media_types()
                .any(|media_type| media_type == "application/*" || media_type == "*/*")
                .then(|| "application/json".to_owned())
        })
}

fn select_request_media_type(
    plan: &simile_schema::BoundRequestBody<'_>,
    requested: Option<&str>,
) -> Option<String> {
    requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| preferred_json_media_type(plan))
        .or_else(|| {
            plan.media_types()
                .find(|media_type| *media_type == "application/x-www-form-urlencoded")
                .map(str::to_owned)
        })
        .or_else(|| plan.media_types().next().map(str::to_owned))
}

fn is_json(media_type: &str) -> bool {
    media_type == "application/json"
        || media_type
            .split_once('/')
            .is_some_and(|(_, subtype)| subtype.ends_with("+json"))
}
