use simile_core::ops;

use crate::cli::{CompileArgs, OutputFormat};
use crate::commands::Status;
use crate::error::Result;
use crate::terminal::Terminal;
use crate::ui::format_count;

pub fn execute(arguments: CompileArgs, terminal: &Terminal) -> Result<Status> {
    let ui = terminal.ui();
    ui.banner()?;
    let contract = arguments.contract.display().to_string();
    let activity = ui.activity(format!("compiling {contract}"))?;
    let result = ops::compile(&ops::CompileRequest {
        contract: arguments.contract,
        contract_format: arguments.contract_format.into(),
        modules: arguments.module,
        artifact: arguments.artifact,
        heuristic_policy: arguments.heuristics.into(),
    });
    let report = match result {
        Ok(report) => {
            activity.complete(format!(
                "compiled {} for {}",
                format_count(report.operations, "operation"),
                report.service
            ))?;
            report
        }
        Err(error) => {
            activity.fail("compilation failed")?;
            return Err(error.into());
        }
    };
    match terminal.output() {
        OutputFormat::Human => {
            ui.result_title(
                "compiled",
                format!("{} {}", report.service, report.service_version),
            )?;
            ui.result_fact(
                "contract",
                format!(
                    "{} · JSON Schema {} · {}",
                    report.contract_format,
                    report.openapi_version,
                    format_count(report.operations, "operation")
                ),
            )?;
            ui.result_fact(
                "behavior",
                format!(
                    "{} · {}",
                    format_count(report.openapi_defaults, "contract default"),
                    format_count(report.hcl_behaviors, "HCL overlay")
                ),
            )?;
            ui.result_fact(
                "runtime",
                format!(
                    "{} · {}",
                    format_count(report.async_operations, "async operation"),
                    format_count(report.backends, "backend")
                ),
            )?;
            ui.result_fact(
                "webhooks",
                format!(
                    "{} · {}",
                    format_count(report.webhooks, "contract"),
                    format_count(report.webhook_emissions, "emission")
                ),
            )?;
            ui.result_fact(
                "faults",
                format!(
                    "{} · {}",
                    format_count(report.faults, "definition"),
                    format_count(report.fault_profiles, "profile")
                ),
            )?;
            ui.result_fact("heuristics", report.heuristic_policy)?;
            if let Some(artifact) = &report.artifact {
                ui.result_fact(
                    "artifact",
                    format!(
                        "{} · format {} · {} bytes",
                        artifact.path.display(),
                        artifact.format_version,
                        artifact.bytes
                    ),
                )?;
                ui.result_fact("digest", format!("sha256:{}", artifact.payload_sha256))?;
            }
        }
        OutputFormat::Json => terminal.json(&report)?,
    }
    Ok(Status::Success)
}
