use serde::{Deserialize, Serialize};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, ResourceError>;

/// Stable failure categories shared by every stack component.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Conflict,
    InvalidCommand,
    InvalidValue,
    KindMismatch,
    NotFound,
    UndeclaredResource,
    Unavailable,
}

/// A public resource failure with a stable code and safe explanation.
#[derive(Clone, Debug, Deserialize, Eq, Error, PartialEq, Serialize)]
#[error("{message}")]
#[serde(rename_all = "camelCase")]
pub struct ResourceError {
    pub code: ErrorCode,
    pub message: String,
}

impl ResourceError {
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
