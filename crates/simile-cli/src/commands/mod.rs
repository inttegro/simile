mod audit;
pub(crate) mod automatic_reporting;
mod compile;
mod conformance;
mod console;
mod describe;
mod doctor;
mod inspect;
mod playground;
mod profile;
mod profiles;
mod proxy;
mod register;
mod remove;
mod replay;
mod report;
mod reporting;
mod reports;
mod run;
mod setup;
mod start;
mod state;
mod test;
mod update;
mod use_profile;
mod version;

use simile_core::Context;

use crate::cli::Command;
use crate::error::Result;
use crate::terminal::Terminal;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Success,
    Failure,
    Code(u8),
}

pub fn execute(command: Command, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    match command {
        Command::Audit(arguments) => audit::execute(arguments, terminal),
        Command::Compile(arguments) => compile::execute(arguments, terminal),
        Command::Console(arguments) => console::execute(arguments, ctx, terminal),
        Command::Conformance(arguments) => conformance::execute(arguments, terminal),
        Command::Doctor => doctor::execute(ctx, terminal),
        Command::Describe(arguments) => describe::execute(arguments, ctx, terminal),
        Command::Inspect(arguments) => inspect::execute(arguments, terminal),
        Command::Playground(arguments) => playground::execute(arguments, terminal),
        Command::Profile(arguments) => profile::execute(arguments, ctx, terminal),
        Command::Profiles(arguments) => profiles::execute(arguments, ctx, terminal),
        Command::Proxy(arguments) => proxy::execute(arguments, ctx, terminal),
        Command::Register(arguments) => register::execute(arguments, ctx, terminal),
        Command::Replay(arguments) => replay::execute(arguments, terminal),
        Command::Report(arguments) => report::execute(arguments, ctx, terminal),
        Command::Reporting(arguments) => reporting::execute(arguments, ctx, terminal),
        Command::Reports(arguments) => reports::execute(arguments, ctx, terminal),
        Command::Remove(arguments) => remove::execute(arguments, ctx, terminal),
        Command::Run(arguments) => run::execute(arguments, terminal),
        Command::Setup(arguments) => setup::execute(arguments, ctx, terminal),
        Command::Start(arguments) => start::execute(arguments, ctx, terminal),
        Command::State(arguments) => state::execute(arguments, ctx, terminal),
        Command::Test(arguments) => test::execute(arguments, ctx, terminal),
        Command::Update(arguments) => update::execute(arguments, ctx, terminal),
        Command::Use(arguments) => use_profile::execute(arguments, ctx, terminal),
        Command::Version => version::execute(ctx, terminal),
    }
}
