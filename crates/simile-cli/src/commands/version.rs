use anstyle::{AnsiColor, Effects, Style};
use simile_core::{Context, ops};

use crate::cli::OutputFormat;
use crate::commands::Status;
use crate::error::Result;
use crate::terminal::Terminal;

const NAME: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let build = ops::version(ctx);
    match terminal.output() {
        OutputFormat::Human => {
            terminal.line(format_args!("{NAME}simile{NAME:#} {}", build.version))?;
            terminal.line(format_args!(
                "{MUTED}target{MUTED:#} {}-{}",
                build.architecture, build.operating_system
            ))?;
            if let Some(commit) = build.commit {
                terminal.line(format_args!("{MUTED}commit{MUTED:#} {commit}"))?;
            }
            Ok(Status::Success)
        }
        OutputFormat::Json => {
            terminal.json(&build)?;
            Ok(Status::Success)
        }
    }
}
