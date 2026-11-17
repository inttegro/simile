use std::io;
use std::net::AddrParseError;
use std::path::PathBuf;
use std::string::FromUtf8Error;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("could not determine the current directory")]
    CurrentDirectory(#[source] io::Error),

    #[error("could not determine the user home directory; set SIMILE_HOME explicitly")]
    UserHome,

    #[error("could not read service source `{path}`: {source}")]
    ReadSource {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("could not read request `{path}`: {source}")]
    ReadRequest {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("could not read request from stdin: {source}")]
    ReadRequestStdin {
        #[source]
        source: io::Error,
    },

    #[error("request from {source_name} is not a valid request object: {source}")]
    InvalidRequest {
        source_name: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("service configuration names `{configured}`, but `{requested}` was requested")]
    ServiceNameMismatch {
        requested: String,
        configured: String,
    },

    #[error(
        "invalid registered service name `{name}`; use lowercase letters or digits followed by lowercase letters, digits, `.`, `_`, or `-`"
    )]
    InvalidRegisteredServiceName { name: String },

    #[error(
        "service `{service}` is not registered; run `simile register {service} --config URL_OR_FILE`"
    )]
    ServiceNotRegistered { service: String },

    #[error("service `{service}` is already registered; pass --force to replace it")]
    ServiceAlreadyRegistered { service: String },

    #[error("service removal not confirmed; --confirm must exactly match `{service}`")]
    ServiceRemovalNotConfirmed { service: String },

    #[error("registered service `{service}` is invalid: {reason}")]
    InvalidRegistration { service: String, reason: String },

    #[error("unsupported Simile registry layout version `{version}` at `{path}`")]
    UnsupportedRegistryLayout { version: String, path: PathBuf },

    #[error("could not {action} registry path `{path}`: {source}")]
    RegistryIo {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("could not decode registry manifest `{path}`: {source}")]
    RegistryManifest {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("service listen host `{host}` is not an IP address: {source}")]
    InvalidListenHost {
        host: String,
        #[source]
        source: AddrParseError,
    },

    #[error("request contains duplicate case-insensitive `{name}` headers")]
    DuplicateRequestHeader { name: String },

    #[error("could not read remote service source: {source}")]
    FetchSource {
        #[source]
        source: ureq::Error,
    },

    #[error("could not fetch service configuration `{url}`: {source}")]
    FetchConfig {
        url: String,
        #[source]
        source: ureq::Error,
    },

    #[error("could not read remote service configuration `{url}`: {source}")]
    ReadRemoteConfig {
        url: String,
        #[source]
        source: io::Error,
    },

    #[error("remote service configuration `{url}` exceeds the {limit}-byte limit")]
    RemoteConfigTooLarge { url: String, limit: u64 },

    #[error("remote service configuration `{url}` is not UTF-8: {source}")]
    RemoteConfigUtf8 {
        url: String,
        #[source]
        source: FromUtf8Error,
    },

    #[error(
        "remote service configuration `{config}` uses {field} path `{value}`; use a URL or an inline declaration"
    )]
    RemoteConfigPath {
        config: String,
        field: &'static str,
        value: String,
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
    ServiceConfig(#[from] simile_hcl::Error),

    #[error(transparent)]
    Compile(#[from] simile_compiler::Error),

    #[error(transparent)]
    Runtime(#[from] simile_runtime::Error),
}
