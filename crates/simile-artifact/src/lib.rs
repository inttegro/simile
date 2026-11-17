#![forbid(unsafe_code)]

mod error;
mod format;

pub use error::{Error, Result};
pub use format::{
    Artifact, ArtifactAsyncOperation, ArtifactFault, ArtifactFaultProfile, ArtifactOperation,
    ArtifactOutcome, ArtifactOutcomeCondition, ArtifactProtocolError, ArtifactResource,
    ArtifactResponse, ArtifactSummary, EXTENSION, FORMAT_VERSION, OperationOutcomeSummary,
    OutcomeSummary, decode, encode,
};
