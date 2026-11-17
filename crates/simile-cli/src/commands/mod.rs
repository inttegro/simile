mod compile;
mod doctor;
mod inspect;
mod register;
mod run;
mod start;
mod update;
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
        Command::Compile(arguments) => compile::execute(arguments, terminal),
        Command::Doctor => doctor::execute(ctx, terminal),
        Command::Inspect(arguments) => inspect::execute(arguments, terminal),
        Command::Register(arguments) => register::execute(arguments, ctx, terminal),
        Command::Run(arguments) => run::execute(arguments, terminal),
        Command::Start(arguments) => start::execute(arguments, ctx, terminal),
        Command::Update(arguments) => update::execute(arguments, ctx, terminal),
        Command::Version => version::execute(ctx, terminal),
    }
}
