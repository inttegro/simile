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

    #[error("could not read request body `{path}`: {source}")]
    ReadRequestBody {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("request body `{path}` is not valid JSON: {source}")]
    InvalidRequestBody {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("could not read remote service source: {source}")]
    FetchSource {
        #[source]
        source: ureq::Error,
    },

    #[error("operation `{operation}` is not present in the compiled service")]
    UnknownOperation { operation: String },

    #[error("operationId `{operation_id}` is not present in the compiled service")]
    UnknownOperationId { operation_id: String },

    #[error("could not read compiled artifact `{path}`: {source}")]
    ReadArtifact {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("could not write compiled artifact `{path}`: {source}")]
    WriteArtifact {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error(transparent)]
    Artifact(#[from] simile_artifact::Error),

    #[error(transparent)]
    Compile(#[from] simile_compiler::Error),

    #[error(transparent)]
    Runtime(#[from] simile_runtime::Error),
}
