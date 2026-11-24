use simile_core::Context;

use crate::cli::{ConsoleArgs, OutputFormat};
use crate::commands::Status;
use crate::console::ConsoleSession;
use crate::error::{Error, Result};
use crate::terminal::Terminal;

pub fn execute(arguments: ConsoleArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    if terminal.output() == OutputFormat::Json {
        return Err(Error::InvalidArguments(
            "console is interactive and does not support --output json",
        ));
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(Error::AsyncRuntime)?;
    let session = runtime.block_on(ConsoleSession::open(ctx.clone(), arguments.service))?;
    if arguments.web {
        runtime.block_on(crate::console::web::run(session, terminal))?;
    } else {
        let mut session = session;
        let result = crate::console::terminal::run(&runtime, &mut session);
        runtime.block_on(session.close());
        result?;
    }
    Ok(Status::Success)
}
