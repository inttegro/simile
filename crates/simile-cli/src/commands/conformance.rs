use anstyle::{AnsiColor, Effects, Style};
use simile_core::ops;

use crate::cli::{ConformanceArgs, OutputFormat};
use crate::commands::Status;
use crate::error::Result;
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const FAILURE: Style = AnsiColor::Red.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: ConformanceArgs, terminal: &Terminal) -> Result<Status> {
    let report = ops::verify_conformance(&ops::ConformanceRequest {
        bundles: arguments.bundles,
    })?;
    match terminal.output() {
        OutputFormat::Json => terminal.json(&report)?,
        OutputFormat::Human => {
            terminal.line(format_args!(
                "{HEADING}Conformance{HEADING:#} {} passed · {} failed",
                report.passed, report.failed
            ))?;
            for case in &report.cases {
                let result = if case.passed {
                    format!("{HEADING}pass{HEADING:#}")
                } else {
                    format!("{FAILURE}fail{FAILURE:#}")
                };
                let operation = case.operation.as_deref().unwrap_or("unknown operation");
                terminal.line(format_args!(
                    "{result} {operation} · {}",
                    case.bundle.display()
                ))?;
                if !case.passed {
                    terminal.line(format_args!("  {MUTED}{}{MUTED:#}", case.message))?;
                }
            }
        }
    }
    Ok(if report.failed == 0 {
        Status::Success
    } else {
        Status::Failure
    })
}
