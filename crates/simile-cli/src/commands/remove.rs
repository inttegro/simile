use anstyle::{AnsiColor, Effects, Style};
use simile_core::{Context, ops};

use crate::cli::{OutputFormat, RemoveArgs};
use crate::commands::Status;
use crate::error::Result;
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: RemoveArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let report = ops::remove(
        ctx,
        &ops::RemoveRequest {
            service: arguments.service,
            confirm: arguments.confirm,
        },
    )?;
    render(&report, terminal)?;
    Ok(Status::Success)
}

fn render(report: &ops::RemoveReport, terminal: &Terminal) -> Result<()> {
    match terminal.output() {
        OutputFormat::Human => {
            terminal.line(format_args!(
                "{HEADING}Removed{HEADING:#} {}",
                report.service
            ))?;
            terminal.line(format_args!(
                "{MUTED}registry{MUTED:#} {}",
                report.directory.display()
            ))?;
            if report.state_removed {
                terminal.line(format_args!("{MUTED}state{MUTED:#} removed"))?;
            }
            if report.exchange_history_removed {
                terminal.line(format_args!("{MUTED}exchange history{MUTED:#} removed"))?;
            }
            Ok(())
        }
        OutputFormat::Json => terminal.json(report),
    }
}
