#![forbid(unsafe_code)]

mod error;
mod format;

pub use error::{Error, Result};
pub use format::{
    Artifact, ArtifactAsyncOperation, ArtifactOperation, ArtifactResource, ArtifactResponse,
    ArtifactSummary, EXTENSION, FORMAT_VERSION, decode, encode,
};
