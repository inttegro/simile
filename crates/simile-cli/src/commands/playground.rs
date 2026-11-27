use crate::cli::{OutputFormat, PlaygroundArgs};
use crate::commands::Status;
use crate::error::{Error, Result};
use crate::terminal::Terminal;

pub fn execute(arguments: PlaygroundArgs, terminal: &Terminal) -> Result<Status> {
    if terminal.output() == OutputFormat::Json {
        return Err(Error::InvalidArguments(
            "playground is interactive and does not support --output json",
        ));
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(Error::AsyncRuntime)?;
    runtime.block_on(crate::playground::run(arguments, terminal))?;
    Ok(Status::Success)
}
