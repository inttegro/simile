use anstyle::{AnsiColor, Effects, Style};
use simile_core::ops;

use crate::cli::{InspectArgs, OutputFormat};
use crate::commands::Status;
use crate::error::Result;
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: InspectArgs, terminal: &Terminal) -> Result<Status> {
    let report = ops::inspect(&ops::InspectRequest {
        artifact: arguments.artifact,
    })?;
    match terminal.output() {
        OutputFormat::Human => {
            terminal.line(format_args!(
                "{HEADING}{}{HEADING:#} {}",
                report.service, report.service_version
            ))?;
            terminal.line(format_args!(
                "{MUTED}artifact{MUTED:#} format {} · {} bytes · compiler {}",
                report.format_version, report.encoded_bytes, report.compiler_version
            ))?;
            terminal.line(format_args!(
                "{MUTED}contract{MUTED:#} OpenAPI {} · {} operations · {} responses",
                report.openapi_version, report.operations, report.responses
            ))?;
            terminal.line(format_args!(
                "{MUTED}runtime{MUTED:#} {} async operations · {} resources · {} value nodes",
                report.async_operations, report.resources, report.value_nodes
            ))?;
            terminal.line(format_args!(
                "{MUTED}digest{MUTED:#} sha256:{}",
                report.payload_sha256
            ))?;
        }
        OutputFormat::Json => terminal.json(&report)?,
    }
    Ok(Status::Success)
}
