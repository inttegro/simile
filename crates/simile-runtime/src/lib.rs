#![forbid(unsafe_code)]

mod auth;
mod codec;
mod effect;
mod execution;
mod fault;
mod operation;
mod outcome;
mod parameter;
mod protocol;
mod replay;
mod response;
mod response_time;
mod webhook;

use serde_json::Value;
use simile_artifact::Artifact;
use simile_schema::{SynthesisError, ValidationIssue};
use thiserror::Error;

pub use auth::{AuthenticatedProfile, authenticate, authenticate_with_profiles};
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

    #[error("request body is not valid `{content_type}` data: {message}")]
    MalformedBody {
        content_type: String,
        message: String,
    },

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

    #[error("operation `{operation}` has no encodable declared response media type")]
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

    #[error("could not synthesize response header `{name}`: {source}")]
    ResponseHeaderSynthesis {
        name: String,
        #[source]
        source: SynthesisError,
    },

    #[error("response header `{name}` failed schema validation with {} issue(s)", .issues.len())]
    InvalidResponseHeader {
        name: String,
        issues: Vec<ValidationIssue>,
    },

    #[error("response header `{name}` must synthesize to a scalar value")]
    EncodeResponseHeader { name: String },

    #[error("could not encode a synthesized JSON request body: {message}")]
    EncodeJson { message: String },

    #[error("could not encode a synthesized `{content_type}` request body: {message}")]
    EncodeBody {
        content_type: String,
        message: String,
    },

    #[error("could not encode synthesized `{content_type}` response body: {message}")]
    EncodeResponseBody {
        content_type: String,
        message: String,
    },

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

    #[error("authentication is required for operation `{operation}`")]
    AuthenticationRequired { operation: String },

    #[error("the authenticated principal cannot call operation `{operation}`")]
    PermissionDenied { operation: String },

    #[error("compiled authentication configuration is invalid: {message}")]
    AuthenticationConfiguration { message: String },
}

impl Error {
    /// Stable diagnostic identity used by logs, reproduction bundles, and verified reports.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownOperation { .. } => "unknown_operation",
            Self::UnknownOutcome { .. } => "unknown_outcome",
            Self::UnknownScenario { .. } => "unknown_scenario",
            Self::UnknownFaultProfile { .. } => "unknown_fault_profile",
            Self::UnknownFault { .. } => "unknown_fault",
            Self::FaultStateUnavailable => "fault_state_unavailable",
            Self::EffectAsyncMissing { .. } => "effect_async_missing",
            Self::EffectWebhookMissing { .. } => "effect_webhook_missing",
            Self::EffectWebhookMediaTypeMissing { .. } => "effect_webhook_media_type_missing",
            Self::InvalidWebhookTarget => "invalid_webhook_target",
            Self::UnsupportedCallbackExpression { .. } => "unsupported_callback_expression",
            Self::CallbackTargetMissing { .. } => "callback_target_missing",
            Self::InvalidWebhookBody { .. } => "invalid_webhook_body",
            Self::EffectProjectionMissing { .. } => "effect_projection_missing",
            Self::BodyNotDeclared { .. } => "body_not_declared",
            Self::BodyRequired { .. } => "body_required",
            Self::MissingContentType { .. } => "missing_content_type",
            Self::UnsupportedMediaType { .. } => "unsupported_media_type",
            Self::UnsupportedEncoding { .. } => "unsupported_encoding",
            Self::MalformedBody { .. } => "malformed_body",
            Self::NoSynthesisMediaType { .. } => "no_synthesis_media_type",
            Self::ResponseNotDeclared { .. } => "response_not_declared",
            Self::ResponseBodyNotDeclared { .. } => "response_body_not_declared",
            Self::EmptyReplayIdentity => "empty_replay_identity",
            Self::ResourceReplayRequiresGet { .. } => "resource_replay_requires_get",
            Self::IdempotencyConflict { .. } => "idempotency_conflict",
            Self::ReplayStateUnavailable => "replay_state_unavailable",
            Self::NoResponseStatus { .. } => "no_response_status",
            Self::RequestContextMismatch { .. } => "request_context_mismatch",
            Self::NoResponseMediaType { .. } => "no_response_media_type",
            Self::UnsupportedResponseMediaType { .. } => "unsupported_response_media_type",
            Self::UnsupportedResponseEncoding { .. } => "unsupported_response_encoding",
            Self::Synthesis { .. } => "request_synthesis_failed",
            Self::ResponseSynthesis { .. } => "response_synthesis_failed",
            Self::ResponseHeaderSynthesis { .. } => "response_header_synthesis_failed",
            Self::InvalidResponseHeader { .. } => "invalid_response_header",
            Self::EncodeResponseHeader { .. } => "encode_response_header_failed",
            Self::EncodeJson { .. } => "encode_json_failed",
            Self::EncodeBody { .. } => "encode_request_body_failed",
            Self::EncodeResponseBody { .. } => "encode_response_body_failed",
            Self::MalformedJson { .. } => "malformed_json",
            Self::InvalidBody { .. } => "invalid_body",
            Self::InvalidParameters { .. } => "invalid_parameters",
            Self::InvalidResponseBody { .. } => "invalid_response_body",
            Self::AuthenticationRequired { .. } => "authentication_required",
            Self::PermissionDenied { .. } => "permission_denied",
            Self::AuthenticationConfiguration { .. } => "authentication_configuration",
        }
    }

    /// Whether an execution failure represents an emulator or contract problem worth verifying.
    /// Caller mistakes remain ordinary emulated responses and never enter automatic reporting.
    #[must_use]
    pub const fn is_reportable(&self) -> bool {
        !matches!(
            self,
            Self::BodyNotDeclared { .. }
                | Self::BodyRequired { .. }
                | Self::MissingContentType { .. }
                | Self::UnsupportedMediaType { .. }
                | Self::UnsupportedEncoding { .. }
                | Self::MalformedBody { .. }
                | Self::EmptyReplayIdentity
                | Self::ResourceReplayRequiresGet { .. }
                | Self::IdempotencyConflict { .. }
                | Self::MalformedJson { .. }
                | Self::InvalidBody { .. }
                | Self::InvalidParameters { .. }
                | Self::AuthenticationRequired { .. }
                | Self::PermissionDenied { .. }
        )
    }

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
    let value = codec::decode(content_type, request.bytes)?;
    let value = if matches!(
        content_type.split(';').next().map(str::trim),
        Some("application/x-www-form-urlencoded" | "multipart/form-data")
    ) {
        coerce_form_fields(value, &selected)
    } else {
        value
    };
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

fn coerce_form_fields(
    mut value: serde_json::Value,
    schema: &simile_schema::BoundMediaType<'_>,
) -> serde_json::Value {
    let Some(fields) = value.as_object_mut() else {
        return value;
    };
    for (name, field) in fields {
        let candidates = form_field_candidates(field);
        if let Some(candidate) = candidates.into_iter().find(|candidate| {
            schema
                .validate_property(name, candidate)
                .is_some_and(|issues| issues.is_empty())
        }) {
            *field = candidate;
        }
    }
    value
}

fn form_field_candidates(value: &serde_json::Value) -> Vec<serde_json::Value> {
    let mut candidates = vec![value.clone()];
    match value {
        serde_json::Value::String(value) => {
            let scalar = wire_scalar(value);
            if scalar != serde_json::Value::String(value.clone()) {
                candidates.push(scalar);
            }
        }
        serde_json::Value::Array(values) => {
            let scalars = serde_json::Value::Array(
                values
                    .iter()
                    .map(|value| match value {
                        serde_json::Value::String(value) => wire_scalar(value),
                        value => value.clone(),
                    })
                    .collect(),
            );
            if scalars != *value {
                candidates.push(scalars);
            }
        }
        _ => {}
    }
    candidates
}

fn wire_scalar(value: &str) -> serde_json::Value {
    match value {
        "true" => serde_json::Value::Bool(true),
        "false" => serde_json::Value::Bool(false),
        "null" => serde_json::Value::Null,
        _ => {
            if let Ok(value) = value.parse::<i64>() {
                return serde_json::Value::from(value);
            }
            if let Ok(value) = value.parse::<u64>() {
                return serde_json::Value::from(value);
            }
            if let Some(value) = value
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
            {
                return serde_json::Value::Number(value);
            }
            serde_json::Value::String(value.to_owned())
        }
    }
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

/// Synthesize the smallest valid transport-neutral request value.
///
/// Setup flows use this form so omitted optional inputs stay omitted. Fuzzed
/// synthesis remains available to callers that are deliberately exploring the
/// contract's wider input space.
pub fn synthesize_minimal_request_value(
    artifact: &Artifact,
    request: SynthesisRequest<'_>,
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
        .synthesize()
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
    let content_type = select_request_media_type(&plan, request.content_type);
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
    let value = selected
        .synthesize_fuzz(seed)
        .map_err(|source| Error::Synthesis { source })?;
    let issues = selected.validate(&value);
    if !issues.is_empty() {
        return Err(Error::InvalidBody { issues });
    }
    let (media_type, bytes) = codec::encode(selected.requested(), &value)?;
    Ok(SynthesizedBody {
        media_type,
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
