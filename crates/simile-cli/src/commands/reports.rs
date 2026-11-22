use anstyle::{AnsiColor, Effects, Style};
use simile_core::{Context, ops};

use crate::cli::{OutputFormat, ReportsArgs};
use crate::commands::Status;
use crate::error::Result;
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: ReportsArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let report = ops::reports(ctx, arguments.service.as_deref())?;
    match terminal.output() {
        OutputFormat::Json => terminal.json(&report)?,
        OutputFormat::Human => {
            terminal.line(format_args!("{HEADING}Reports{HEADING:#}"))?;
            if report.reports.is_empty() {
                terminal.line(format_args!("{MUTED}No reports prepared.{MUTED:#}"))?;
            }
            for report in report.reports {
                terminal.line(format_args!(
                    "{} · {} · {:?} · {}",
                    report.id, report.service, report.status, report.title
                ))?;
            }
        }
    }
    Ok(Status::Success)
}
