#![forbid(unsafe_code)]

mod error;
mod format;

pub use error::{Error, Result};
pub use format::{
    Artifact, ArtifactAsyncOperation, ArtifactCredential, ArtifactDescription, ArtifactEffect,
    ArtifactFault, ArtifactFaultProfile, ArtifactOperation, ArtifactOutcome,
    ArtifactOutcomeCondition, ArtifactParameter, ArtifactPrincipal, ArtifactProfile,
    ArtifactProtocolError, ArtifactResource, ArtifactResponse, ArtifactScenario,
    ArtifactSecurityRequirement, ArtifactSecurityScheme, ArtifactSummary, EXTENSION,
    FORMAT_VERSION, OperationOutcomeSummary, OutcomeSummary, decode, encode,
};
