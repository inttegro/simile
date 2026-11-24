#![forbid(unsafe_code)]

mod cli;
mod commands;
mod console;
mod error;
mod terminal;
mod ui;

use std::process::ExitCode;

use anstream::AutoStream;
use clap::Parser;
use cli::Cli;
use error::Result;
use simile_core::{BuildInfo, Context};
use terminal::Terminal;
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let terminal = Terminal::new(cli.output, cli.color, cli.verbose > 0, cli.quiet);

    match run(cli, &terminal) {
        Ok(commands::Status::Success) => ExitCode::SUCCESS,
        Ok(commands::Status::Failure) => ExitCode::FAILURE,
        Ok(commands::Status::Code(code)) => ExitCode::from(code),
        Err(error) if error.is_broken_pipe() => ExitCode::SUCCESS,
        Err(error) => {
            drop(terminal.error(&error));
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli, terminal: &Terminal) -> Result<commands::Status> {
    let runs_server = matches!(
        &cli.command,
        cli::Command::Console(_)
            | cli::Command::Proxy(_)
            | cli::Command::Start(_)
            | cli::Command::Test(_)
    );
    init_tracing(cli.verbose, cli.quiet, runs_server, terminal);
    let build = BuildInfo::new(
        env!("CARGO_PKG_VERSION"),
        option_env!("SIMILE_BUILD_COMMIT"),
        std::env::consts::ARCH,
        std::env::consts::OS,
    );
    let ctx = Context::from_env(build)?;
    commands::execute(cli.command, &ctx, terminal)
}

fn init_tracing(verbosity: u8, quiet: bool, runs_server: bool, terminal: &Terminal) {
    let fallback = if quiet {
        "off"
    } else {
        match verbosity {
            0 if runs_server => "info",
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        }
    };
    let filter = EnvFilter::try_from_env("SIMILE_LOG").unwrap_or_else(|_| EnvFilter::new(fallback));
    let color = terminal.color_choice();
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .event_format(ui::trace::TimelineFormatter::new(verbosity))
        .with_writer(move || AutoStream::new(std::io::stderr(), color))
        .try_init();
    drop(subscriber);
}
