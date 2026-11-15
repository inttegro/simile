use thiserror::Error;

pub type Result<T> = std::result::Result<T, CompileError>;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CompileError {
    #[error("schema `{field}` must be {expected}")]
    InvalidField {
        field: String,
        expected: &'static str,
    },

    #[error("schema `{field}` uses unsupported reference `{reference}`")]
    UnsupportedReference { field: String, reference: String },

    #[error("schema `{field}` references unknown component `{reference}`")]
    UnknownReference { field: String, reference: String },

    #[error("compiled schema contains too many nodes")]
    TooManyNodes,

    #[error("compiled schema contains an unfinished node")]
    IncompleteNode,
}
