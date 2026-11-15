use std::io;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Core(#[from] simile_core::Error),

    #[error("could not write command output")]
    Output(#[source] io::Error),

    #[error("could not serialize command output")]
    Serialize(#[source] serde_json::Error),
}

impl Error {
    #[must_use]
    pub fn is_broken_pipe(&self) -> bool {
        match self {
            Self::Output(error) => error.kind() == io::ErrorKind::BrokenPipe,
            Self::Serialize(error) => error.io_error_kind() == Some(io::ErrorKind::BrokenPipe),
            Self::Core(_) => false,
        }
    }
}
