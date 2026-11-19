use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("TypeScript declaration `{name}` could not be parsed: {detail}")]
    Parse { name: String, detail: String },
    #[error("unsupported TypeScript contract type in `{name}`: {detail}")]
    Unsupported { name: String, detail: String },
    #[error("TypeScript contract is missing `{0}`")]
    Missing(&'static str),
    #[error("TypeScript contract declaration `{0}` was not found")]
    UnknownDeclaration(String),
    #[error("TypeScript operation `{operation}` is invalid: {detail}")]
    InvalidOperation { operation: String, detail: String },
    #[error("duplicate TypeScript operation ID `{0}`")]
    DuplicateOperation(String),
    #[error("TypeScript soundness declaration `{owner}` is invalid: {detail}")]
    InvalidConstraint { owner: String, detail: String },
}
