use std::path::{Path, PathBuf};

use crate::{BuildInfo, Error, Result};

/// Shared application state made explicit for operations and tests.
#[derive(Clone, Debug)]
pub struct Context {
    build: BuildInfo,
    current_dir: PathBuf,
    simile_home: Option<PathBuf>,
}

impl Context {
    /// Build a context from the current process environment.
    pub fn from_env(build: BuildInfo) -> Result<Self> {
        let current_dir = std::env::current_dir().map_err(Error::CurrentDirectory)?;
        let simile_home = std::env::var_os("SIMILE_HOME")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .map(|path| resolve(&current_dir, path))
            .or_else(|| user_home().map(|home| home.join(".simile")));
        Ok(Self {
            build,
            current_dir,
            simile_home,
        })
    }

    /// Build a context with an explicit directory, primarily for tests and embedding.
    #[must_use]
    pub fn new(build: BuildInfo, current_dir: impl Into<PathBuf>) -> Self {
        let current_dir = current_dir.into();
        Self {
            build,
            simile_home: Some(current_dir.join(".simile")),
            current_dir,
        }
    }

    /// Build a context with an explicit Simile home, primarily for tests and embedding.
    #[must_use]
    pub fn with_simile_home(
        build: BuildInfo,
        current_dir: impl Into<PathBuf>,
        simile_home: impl Into<PathBuf>,
    ) -> Self {
        Self {
            build,
            current_dir: current_dir.into(),
            simile_home: Some(simile_home.into()),
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

    pub fn simile_home(&self) -> Result<&Path> {
        self.simile_home.as_deref().ok_or(Error::UserHome)
    }
}

fn resolve(current_dir: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        current_dir.join(path)
    }
}

fn user_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}
