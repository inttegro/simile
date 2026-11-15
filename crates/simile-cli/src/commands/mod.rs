mod doctor;
mod version;

use simile_core::Context;

use crate::cli::Command;
use crate::error::Result;
use crate::terminal::Terminal;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Success,
    Failure,
}

pub fn execute(command: Command, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    match command {
        Command::Doctor => doctor::execute(ctx, terminal),
        Command::Version => version::execute(ctx, terminal),
    }
}
