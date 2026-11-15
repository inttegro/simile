#![forbid(unsafe_code)]

use serde_json::Value;
use simile_artifact::Artifact;
use simile_schema::ValidationIssue;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

/// The request-body portion of an incoming HTTP request.
#[derive(Clone, Copy, Debug)]
pub struct IncomingBody<'a> {
    pub operation: &'a str,
    pub content_type: Option<&'a str>,
    pub bytes: &'a [u8],
}

/// A body that has been decoded and checked against its compiled OpenAPI schema.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedBody {
    media_type: String,
    value: Value,
}

impl ValidatedBody {
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

    #[error("request body is not valid JSON at line {line}, column {column}: {message}")]
    MalformedJson {
        line: usize,
        column: usize,
        message: String,
    },

    #[error("request body failed schema validation with {} issue(s)", .issues.len())]
    InvalidBody { issues: Vec<ValidationIssue> },
}

impl Error {
    #[must_use]
    pub fn issues(&self) -> &[ValidationIssue] {
        match self {
            Self::InvalidBody { issues } => issues,
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
        media_type: selected.requested().to_owned(),
        value,
    }))
}

fn is_json(media_type: &str) -> bool {
    media_type == "application/json"
        || media_type
            .split_once('/')
            .is_some_and(|(_, subtype)| subtype.ends_with("+json"))
}
