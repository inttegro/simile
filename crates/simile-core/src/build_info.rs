use serde::Serialize;

/// Metadata identifying the Simile binary that is currently running.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    pub architecture: String,
    pub operating_system: String,
}

impl BuildInfo {
    #[must_use]
    pub fn new(
        version: impl Into<String>,
        commit: Option<impl Into<String>>,
        architecture: impl Into<String>,
        operating_system: impl Into<String>,
    ) -> Self {
        Self {
            version: version.into(),
            commit: commit.map(Into::into),
            architecture: architecture.into(),
            operating_system: operating_system.into(),
        }
    }
}
