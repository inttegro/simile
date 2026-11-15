use anstyle::{AnsiColor, Effects, Style};
use simile_core::{Context, ops};

use crate::cli::OutputFormat;
use crate::commands::Status;
use crate::error::Result;
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const PASS: Style = AnsiColor::Green.on_default();
const FAIL: Style = AnsiColor::Red.on_default();

pub fn execute(ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let report = ops::doctor(ctx);
    match terminal.output() {
        OutputFormat::Human => {
            terminal.line(format_args!("{HEADING}Simile doctor{HEADING:#}"))?;
            for check in report.checks {
                let (symbol, style) = match check.status {
                    ops::CheckStatus::Pass => ("ok", PASS),
                    ops::CheckStatus::Fail => ("fail", FAIL),
                };
                terminal.line(format_args!(
                    "{style}{symbol:>4}{style:#}  {:<20} {}",
                    check.name, check.detail
                ))?;
            }
            Ok(if report.ok {
                Status::Success
            } else {
                Status::Failure
            })
        }
        OutputFormat::Json => {
            terminal.json(&report)?;
            Ok(if report.ok {
                Status::Success
            } else {
                Status::Failure
            })
        }
    }
}
