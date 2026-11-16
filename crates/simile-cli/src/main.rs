#![forbid(unsafe_code)]

mod cli;
mod commands;
mod error;
mod terminal;

use std::process::ExitCode;

use clap::Parser;
use cli::Cli;
use error::Result;
use simile_core::{BuildInfo, Context};
use terminal::Terminal;
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let terminal = Terminal::new(cli.output, cli.color, cli.verbose > 0);

    match run(cli, &terminal) {
        Ok(commands::Status::Success) => ExitCode::SUCCESS,
        Ok(commands::Status::Failure) => ExitCode::FAILURE,
        Err(error) if error.is_broken_pipe() => ExitCode::SUCCESS,
        Err(error) => {
            drop(terminal.error(&error));
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli, terminal: &Terminal) -> Result<commands::Status> {
    init_tracing(cli.verbose, cli.quiet);
    let build = BuildInfo::new(
        env!("CARGO_PKG_VERSION"),
        option_env!("SIMILE_BUILD_COMMIT"),
        std::env::consts::ARCH,
        std::env::consts::OS,
    );
    let ctx = Context::from_env(build)?;
    commands::execute(cli.command, &ctx, terminal)
}

fn init_tracing(verbosity: u8, quiet: bool) {
    let fallback = if quiet {
        "off"
    } else {
        match verbosity {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        }
    };
    let filter = EnvFilter::try_from_env("SIMILE_LOG").unwrap_or_else(|_| EnvFilter::new(fallback));
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .without_time()
        .try_init();
    drop(subscriber);
}
