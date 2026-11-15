use std::fs;
use std::path::PathBuf;

use simile_artifact::ArtifactSummary;

use crate::{Error, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectRequest {
    pub artifact: PathBuf,
}

/// Load, verify, and summarize a compiled Simile artifact.
pub fn inspect(request: &InspectRequest) -> Result<ArtifactSummary> {
    let encoded = fs::read(&request.artifact).map_err(|source| Error::ReadArtifact {
        path: request.artifact.clone(),
        source,
    })?;
    Ok(simile_artifact::decode(&encoded)?.summary())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_artifact_input() {
        let path = std::env::temp_dir().join(format!(
            "simile-core-inspect-{}.similec",
            std::process::id()
        ));
        fs::write(&path, b"not an artifact").unwrap_or_else(|error| panic!("{error}"));

        let error = match inspect(&InspectRequest {
            artifact: path.clone(),
        }) {
            Ok(_) => panic!("input should be rejected"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            Error::Artifact(simile_artifact::Error::Truncated)
        ));
        fs::remove_file(path).unwrap_or_else(|error| panic!("{error}"));
    }
}
