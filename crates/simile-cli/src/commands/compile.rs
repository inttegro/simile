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
        artifact: arguments.artifact,
        heuristic_policy: arguments.heuristics.into(),
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
            terminal.line(format_args!(
                "{MUTED}faults{MUTED:#} {} definitions · {} profiles",
                report.faults, report.fault_profiles
            ))?;
            terminal.line(format_args!(
                "{MUTED}heuristics{MUTED:#} {}",
                report.heuristic_policy
            ))?;
            if let Some(artifact) = &report.artifact {
                terminal.line(format_args!(
                    "{MUTED}artifact{MUTED:#} {} · format {} · {} bytes",
                    artifact.path.display(),
                    artifact.format_version,
                    artifact.bytes
                ))?;
                terminal.line(format_args!(
                    "{MUTED}digest{MUTED:#} sha256:{}",
                    artifact.payload_sha256
                ))?;
            }
        }
        OutputFormat::Json => terminal.json(&report)?,
    }
    Ok(Status::Success)
}
