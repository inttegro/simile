#![forbid(unsafe_code)]

mod operation;
mod response;

use serde_json::Value;
use simile_artifact::Artifact;
use simile_schema::{SynthesisError, ValidationIssue};
use thiserror::Error;

pub use operation::{
    ExecutionOptions, OperationCall, OperationOutcome, PlannedAsyncOperation, Runtime,
};
pub use response::{ResponseSynthesisRequest, SynthesizedResponse, synthesize_response};

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

/// A deterministic request body synthesized from a compiled OpenAPI contract.
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

/// A body that has been decoded and checked against its compiled OpenAPI schema.
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

    #[error("operation `{operation}` has no declared JSON media type to synthesize")]
    NoSynthesisMediaType {
        operation: String,
        expected: Vec<String>,
    },

    #[error("operation `{operation}` does not declare a response matching HTTP {status}")]
    ResponseNotDeclared { operation: String, status: u16 },

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

    #[error("synthesized response body failed schema validation with {} issue(s)", .issues.len())]
    InvalidResponseBody { issues: Vec<ValidationIssue> },
}

impl Error {
    #[must_use]
    pub fn issues(&self) -> &[ValidationIssue] {
        match self {
            Self::InvalidBody { issues } | Self::InvalidResponseBody { issues } => issues,
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

/// Synthesize a deterministic JSON body and verify it against the same compiled request schema.
pub fn synthesize_request_body(
    artifact: &Artifact,
    request: SynthesisRequest<'_>,
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
        .synthesize()
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

fn is_json(media_type: &str) -> bool {
    media_type == "application/json"
        || media_type
            .split_once('/')
            .is_some_and(|(_, subtype)| subtype.ends_with("+json"))
}
