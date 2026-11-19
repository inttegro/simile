use anstyle::{AnsiColor, Effects, Style};
use simile_core::ops;

use crate::cli::{OutputFormat, ReplayArgs};
use crate::commands::Status;
use crate::error::Result;
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: ReplayArgs, terminal: &Terminal) -> Result<Status> {
    let report = ops::replay(&ops::ReplayRequest {
        bundle: arguments.bundle,
    })?;
    match terminal.output() {
        OutputFormat::Json => terminal.json(&report)?,
        OutputFormat::Human => {
            let label = if report.matched {
                "Reproduced"
            } else {
                "Diverged"
            };
            terminal.line(format_args!(
                "{HEADING}{label}{HEADING:#} {} · seed {}",
                report.operation, report.seed
            ))?;
            terminal.line(format_args!(
                "{MUTED}service{MUTED:#} {} {}",
                report.service, report.service_version
            ))?;
            if !report.matched {
                terminal.line(format_args!(
                    "{MUTED}expected{MUTED:#} {:?}",
                    report.expected
                ))?;
                terminal.line(format_args!("{MUTED}actual{MUTED:#} {:?}", report.actual))?;
            }
        }
    }
    Ok(if report.matched {
        Status::Success
    } else {
        Status::Failure
    })
}
