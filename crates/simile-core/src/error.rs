use std::io;
use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("could not determine the current directory")]
    CurrentDirectory(#[source] io::Error),

    #[error("could not read service source `{path}`: {source}")]
    ReadSource {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error(transparent)]
    Compile(#[from] simile_compiler::Error),
}
