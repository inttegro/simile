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
                "{MUTED}fidelity{MUTED:#} {} outcomes · {} provider-authored · {} schema-synthesized · {} scenario selections · {} underspecified operations",
                report.outcomes,
                report.provider_authored_outcomes,
                report.schema_synthesized_outcomes,
                report.scenario_selections,
                report.underspecified_operations
            ))?;
            terminal.line(format_args!(
                "{MUTED}faults{MUTED:#} {} definitions · {} profiles · {} scenario injections",
                report.faults, report.fault_profiles, report.scenario_faults
            ))?;
            terminal.line(format_args!(
                "{MUTED}export{MUTED:#} {}",
                if report.public_export_safe {
                    "public-safe"
                } else {
                    "review required"
                }
            ))?;
            for operation in &report.operation_outcomes {
                terminal.line(format_args!(
                    "{MUTED}operation{MUTED:#} {}{}",
                    operation.operation,
                    if operation.underspecified {
                        " · underspecified"
                    } else {
                        ""
                    }
                ))?;
                for outcome in &operation.outcomes {
                    terminal.line(format_args!(
                        "  {outcome_name} · HTTP {status} · {provenance:?}",
                        outcome_name = outcome.name,
                        status = outcome.status,
                        provenance = outcome.provenance
                    ))?;
                }
            }
            terminal.line(format_args!(
                "{MUTED}digest{MUTED:#} sha256:{}",
                report.payload_sha256
            ))?;
        }
        OutputFormat::Json => terminal.json(&report)?,
    }
    Ok(Status::Success)
}
