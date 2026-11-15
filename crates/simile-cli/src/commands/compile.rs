use anstyle::{AnsiColor, Effects, Style};
use simile_core::ops;

use crate::cli::{CompileArgs, OutputFormat};
use crate::commands::Status;
use crate::error::Result;
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: CompileArgs, terminal: &Terminal) -> Result<Status> {
    let report = ops::compile(&ops::CompileRequest {
        openapi: arguments.openapi,
        behaviors: arguments.behavior,
    })?;
    match terminal.output() {
        OutputFormat::Human => {
            terminal.line(format_args!(
                "{HEADING}Compiled{HEADING:#} {} {}",
                report.service, report.service_version
            ))?;
            terminal.line(format_args!(
                "{MUTED}contract{MUTED:#} OpenAPI {} · {} operations",
                report.openapi_version, report.operations
            ))?;
            terminal.line(format_args!(
                "{MUTED}behavior{MUTED:#} {} OpenAPI defaults · {} HCL overlays",
                report.openapi_defaults, report.hcl_behaviors
            ))?;
            terminal.line(format_args!(
                "{MUTED}runtime{MUTED:#} {} async operations · {} resources",
                report.async_operations, report.resources
            ))?;
        }
        OutputFormat::Json => terminal.json(&report)?,
    }
    Ok(Status::Success)
}
