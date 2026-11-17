use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("could not compile a body schema: {0}")]
    Schema(#[from] simile_schema::CompileError),

    #[error("could not encode the compiled artifact: {0}")]
    Encode(String),

    #[error("could not decode the compiled artifact: {0}")]
    Decode(String),

    #[error("the file is not a Simile compiled artifact")]
    InvalidMagic,

    #[error("compiled artifact format {0} is not supported")]
    UnsupportedFormat(u16),

    #[error("compiled artifact flags {0:#06x} are not supported")]
    UnsupportedFlags(u16),

    #[error("the compiled artifact is truncated")]
    Truncated,

    #[error("the compiled artifact has trailing bytes")]
    TrailingBytes,

    #[error("the compiled artifact failed its integrity check")]
    Integrity,

    #[error("the compiled artifact is too large")]
    TooLarge,

    #[error("the compiled service contains too many {0}")]
    Limit(&'static str),

    #[error("compiled artifact references unknown {kind} `{name}`")]
    UnknownReference { kind: &'static str, name: String },

    #[error("public artifact export rejected private implementation marker `{0}`")]
    UnsafePublicExport(String),

    #[error(
        "outcome `{outcome}` for `{operation}` is not valid against its OpenAPI response: {issues}"
    )]
    InvalidOutcomeBody {
        operation: String,
        outcome: String,
        issues: String,
    },

    #[error("compiled artifact contains an invalid {0}")]
    Invalid(&'static str),
}
