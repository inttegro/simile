#![forbid(unsafe_code)]

mod error;
mod format;

pub use error::{Error, Result};
pub use format::{
    Artifact, ArtifactAsyncOperation, ArtifactDescription, ArtifactEffect, ArtifactFault,
    ArtifactFaultProfile, ArtifactOperation, ArtifactOutcome, ArtifactOutcomeCondition,
    ArtifactParameter, ArtifactProtocolError, ArtifactResource, ArtifactResponse, ArtifactScenario,
    ArtifactSummary, EXTENSION, FORMAT_VERSION, OperationOutcomeSummary, OutcomeSummary, decode,
    encode,
};
