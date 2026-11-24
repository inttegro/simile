use std::io;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid command arguments: {0}")]
    InvalidArguments(&'static str),

    #[error("invalid setup assignment `{0}`; use NAME=VALUE")]
    InvalidSetupAssignment(String),

    #[error("setup does not collect a value named `{0}`")]
    UnknownSetupField(String),

    #[error("setup value `{0}` was supplied more than once")]
    DuplicateSetupField(String),

    #[error("setup value `{name}` must be {expected}")]
    InvalidSetupValue {
        name: String,
        expected: &'static str,
    },

    #[error("could not read setup input")]
    Prompt(#[source] io::Error),

    #[error(transparent)]
    Core(#[from] simile_core::Error),

    #[error("could not write command output")]
    Output(#[source] io::Error),

    #[error("could not serialize command output")]
    Serialize(#[source] serde_json::Error),

    #[error("could not initialize the service runtime")]
    AsyncRuntime(#[source] io::Error),

    #[error("the interactive console requires a terminal; pass --web to use the browser console")]
    ConsoleTerminalRequired,

    #[error("could not read an interactive console event")]
    ConsoleEvent(#[source] io::Error),

    #[error("could not render the interactive console")]
    ConsoleRender(#[source] io::Error),

    #[error("no registered services; run `simile register NAME --config URL_OR_FILE` first")]
    NoRegisteredServices,

    #[error("{address} is already in use by something other than registered service `{service}`")]
    ConsoleServiceConflict {
        service: String,
        address: std::net::SocketAddr,
    },

    #[error(transparent)]
    Server(#[from] simile_server::Error),

    #[error(transparent)]
    Learning(#[from] simile_learning::Error),
}

impl Error {
    #[must_use]
    pub fn is_broken_pipe(&self) -> bool {
        match self {
            Self::Output(error) => error.kind() == io::ErrorKind::BrokenPipe,
            Self::Serialize(error) => error.io_error_kind() == Some(io::ErrorKind::BrokenPipe),
            Self::InvalidArguments(_)
            | Self::InvalidSetupAssignment(_)
            | Self::UnknownSetupField(_)
            | Self::DuplicateSetupField(_)
            | Self::InvalidSetupValue { .. }
            | Self::Prompt(_)
            | Self::Core(_)
            | Self::AsyncRuntime(_)
            | Self::ConsoleTerminalRequired
            | Self::ConsoleEvent(_)
            | Self::ConsoleRender(_)
            | Self::NoRegisteredServices
            | Self::ConsoleServiceConflict { .. }
            | Self::Server(_)
            | Self::Learning(_) => false,
        }
    }
}
