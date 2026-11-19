use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("the Smithy parser could not be initialized")]
    ParserInitialization,
    #[error("the Smithy document could not be parsed")]
    Parse,
    #[error("Smithy syntax error at line {line}, column {column}")]
    Syntax { line: usize, column: usize },
    #[error("invalid Smithy {context}: {detail}")]
    Invalid {
        context: &'static str,
        detail: String,
    },
    #[error("Smithy service shape is missing")]
    MissingService,
    #[error("Smithy service references unknown operation `{0}`")]
    UnknownOperation(String),
    #[error("Smithy operation `{operation}` is missing `{trait_name}`")]
    MissingOperationTrait {
        operation: String,
        trait_name: &'static str,
    },
    #[error("duplicate Smithy operation ID `{operation_id}`")]
    DuplicateOperationId { operation_id: String },
    #[error("Smithy shape `{0}` was not declared")]
    UnknownShape(String),
    #[error("Smithy operation `{operation}` has invalid HTTP status `{status}`")]
    InvalidStatus { operation: String, status: i64 },
}
