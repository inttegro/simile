use anstyle::{AnsiColor, Effects, Style};
use simile_core::ops;

use crate::cli::{AuditArgs, OutputFormat};
use crate::commands::Status;
use crate::error::Result;
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const ERROR: Style = AnsiColor::Red.on_default().effects(Effects::BOLD);
const WARNING: Style = AnsiColor::Yellow.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: AuditArgs, terminal: &Terminal) -> Result<Status> {
    let report = ops::audit(&ops::AuditRequest {
        artifact: arguments.artifact,
    })?;
    match terminal.output() {
        OutputFormat::Json => terminal.json(&report)?,
        OutputFormat::Human => render(&report, terminal)?,
    }
    let failed = report.errors > 0 || (arguments.strict && report.warnings > 0);
    Ok(if failed {
        Status::Failure
    } else {
        Status::Success
    })
}

fn render(report: &ops::AuditReport, terminal: &Terminal) -> Result<()> {
    terminal.line(format_args!(
        "{HEADING}Fidelity audit{HEADING:#} {} {}",
        report.service, report.service_version
    ))?;
    terminal.line(format_args!(
        "{MUTED}coverage{MUTED:#} {} operations · {} authored outcomes · {} authored effects · {} inferred effects",
        report.operations,
        report.provider_authored_outcomes,
        report.provider_authored_effects,
        report.convention_inferred_effects
    ))?;
    terminal.line(format_args!(
        "{MUTED}findings{MUTED:#} {} errors · {} warnings",
        report.errors, report.warnings
    ))?;
    for finding in &report.findings {
        let severity = match finding.severity {
            ops::AuditSeverity::Error => format!("{ERROR}error{ERROR:#}"),
            ops::AuditSeverity::Warning => format!("{WARNING}warning{WARNING:#}"),
            ops::AuditSeverity::Info => format!("{MUTED}info{MUTED:#}"),
        };
        terminal.line(format_args!(
            "{severity} {} · {}",
            finding.code, finding.subject
        ))?;
        terminal.line(format_args!("  {}", finding.message))?;
        terminal.line(format_args!(
            "  {MUTED}next{MUTED:#} {}",
            finding.remediation
        ))?;
    }
    terminal.line(format_args!(
        "{MUTED}digest{MUTED:#} sha256:{}",
        report.artifact_sha256
    ))?;
    Ok(())
}
