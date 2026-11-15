use semver::Error as SemverError;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("the input is neither valid OpenAPI JSON nor YAML")]
    Document {
        #[source]
        json: serde_json::Error,
        yaml: serde_norway::Error,
    },

    #[error("the OpenAPI document has no `openapi` version field")]
    MissingVersion,

    #[error("`{value}` is not a valid OpenAPI semantic version")]
    InvalidVersion {
        value: String,
        #[source]
        source: SemverError,
    },

    #[error("OpenAPI {value} is unsupported; Simile accepts the 3.0, 3.1, and 3.2 feature lines")]
    UnsupportedVersion { value: String },

    #[error("OpenAPI `{field}` must be {expected}")]
    InvalidField {
        field: String,
        expected: &'static str,
    },

    #[error("operation {method} {path} must declare at least one response")]
    MissingResponses { method: String, path: String },

    #[error("operation {method} {path} declares invalid response status `{status}`")]
    InvalidResponseStatus {
        method: String,
        path: String,
        status: String,
    },

    #[error("operationId `{operation_id}` is used by both {first} and {second}")]
    DuplicateOperationId {
        operation_id: String,
        first: String,
        second: String,
    },

    #[error("additional operation `{method}` duplicates a standard Path Item operation")]
    DuplicateStandardMethod { method: String },
}
