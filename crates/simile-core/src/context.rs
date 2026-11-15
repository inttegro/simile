use std::path::{Path, PathBuf};

use crate::{BuildInfo, Error, Result};

/// Shared application state made explicit for operations and tests.
#[derive(Clone, Debug)]
pub struct Context {
    build: BuildInfo,
    current_dir: PathBuf,
}

impl Context {
    /// Build a context from the current process environment.
    pub fn from_env(build: BuildInfo) -> Result<Self> {
        let current_dir = std::env::current_dir().map_err(Error::CurrentDirectory)?;
        Ok(Self { build, current_dir })
    }

    /// Build a context with an explicit directory, primarily for tests and embedding.
    #[must_use]
    pub fn new(build: BuildInfo, current_dir: impl Into<PathBuf>) -> Self {
        Self {
            build,
            current_dir: current_dir.into(),
        }
    }

    #[must_use]
    pub const fn build(&self) -> &BuildInfo {
        &self.build
    }

    #[must_use]
    pub fn current_dir(&self) -> &Path {
        &self.current_dir
    }
}
