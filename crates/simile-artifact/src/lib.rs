#![forbid(unsafe_code)]

mod error;
mod format;

pub use error::{Error, Result};
pub use format::{
    Artifact, ArtifactAsyncOperation, ArtifactDescription, ArtifactFault, ArtifactFaultProfile,
    ArtifactOperation, ArtifactOutcome, ArtifactOutcomeCondition, ArtifactProtocolError,
    ArtifactResource, ArtifactResponse, ArtifactScenario, ArtifactSummary, EXTENSION,
    FORMAT_VERSION, OperationOutcomeSummary, OutcomeSummary, decode, encode,
};
