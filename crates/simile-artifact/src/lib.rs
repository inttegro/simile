#![forbid(unsafe_code)]

mod error;
mod format;

pub use error::{Error, Result};
pub use format::{
    Artifact, ArtifactOperation, ArtifactResponse, ArtifactSummary, EXTENSION, FORMAT_VERSION,
    decode, encode,
};
