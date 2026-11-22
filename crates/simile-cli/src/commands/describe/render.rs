mod details;

use anstyle::{AnsiColor, Effects, Style};
use simile_core::ops;

use crate::cli::OutputFormat;
use crate::error::Result;
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();
const FIELD_WIDTH: usize = 20;

pub(super) fn service(report: &ops::DescribeServiceReport, terminal: &Terminal) -> Result<()> {
    if terminal.output() == OutputFormat::Json {
        return terminal.json(report);
    }

    header(report, terminal)?;
    overview(report, terminal)?;

    if terminal.is_verbose() {
        details::render(report, terminal)?;
    } else {
        terminal.line(format_args!(""))?;
        terminal.line(format_args!(
            "{MUTED}Use --verbose to list named objects and operation outcomes.{MUTED:#}"
        ))?;
        terminal.line(format_args!(
            "{MUTED}Use --output json for machine-readable output.{MUTED:#}"
        ))?;
    }

    Ok(())
}

fn header(report: &ops::DescribeServiceReport, terminal: &Terminal) -> Result<()> {
    terminal.line(format_args!(
        "{HEADING}{}{HEADING:#} {MUTED}{}{MUTED:#}",
        report.service, report.service_version
    ))?;
    if let Some(description) = &report.description {
        terminal.line(format_args!("{description}"))?;
    }
    Ok(())
}

fn overview(report: &ops::DescribeServiceReport, terminal: &Terminal) -> Result<()> {
    section(terminal, "Runtime")?;
    fact(terminal, "Listen", format!("http://{}", report.listen))?;
    fact(terminal, "Source", &report.source)?;
    if let Some(revision) = &report.revision {
        fact(terminal, "Revision", revision)?;
    }
    if let Some(endpoint) = &report.reporting_endpoint {
        fact(terminal, "Issue reports", endpoint)?;
    }

    section(terminal, "Contract")?;
    fact(
        terminal,
        "Format",
        format!("OpenAPI {}", report.openapi_version),
    )?;
    fact(terminal, "Operations", report.operation_count)?;
    fact(
        terminal,
        "Webhook contracts",
        report.webhook_contracts.len(),
    )?;
    if !report.webhook_contracts.is_empty() {
        fact(
            terminal,
            "Webhook delivery",
            format!(
                "{} · {} reorder · {} window · {} workers",
                readable_identifier(&report.webhooks.ordering),
                percentage(report.webhooks.out_of_order_probability_ppm),
                duration(report.webhooks.reorder_window_milliseconds),
                report.webhooks.concurrency
            ),
        )?;
    }

    section(terminal, "Service model")?;
    fact(terminal, "Principals", report.principals.len())?;
    fact(terminal, "Service profiles", report.profiles.len())?;
    fact(terminal, "Setup workflows", report.principal_setups.len())?;
    fact(terminal, "Scenarios", report.scenarios.len())?;
    fact(terminal, "Faults", report.faults.len())?;
    fact(terminal, "Fault profiles", report.fault_profiles.len())?;
    fact(terminal, "Protocol errors", report.protocol_errors.len())?;
    fact(terminal, "Backends", report.backends.len())?;
    fact(terminal, "Async operations", report.async_operations.len())?;
    fact(
        terminal,
        "Error/outcome docs",
        report.authoring_objects.len(),
    )?;
    Ok(())
}

fn section(terminal: &Terminal, title: &str) -> Result<()> {
    terminal.line(format_args!(""))?;
    terminal.line(format_args!("{HEADING}{title}{HEADING:#}"))
}

fn fact(terminal: &Terminal, label: &str, value: impl std::fmt::Display) -> Result<()> {
    terminal.line(format_args!(
        "  {MUTED}{label:<FIELD_WIDTH$}{MUTED:#} {value}"
    ))
}

pub(super) fn percentage(parts_per_million: u32) -> String {
    let mut value = format!("{:.4}", f64::from(parts_per_million) / 10_000.0);
    while value.ends_with('0') {
        value.pop();
    }
    if value.ends_with('.') {
        value.pop();
    }
    format!("{value}%")
}

fn duration(milliseconds: u64) -> String {
    if milliseconds % 1_000 == 0 {
        format!("{}s", milliseconds / 1_000)
    } else {
        format!("{milliseconds}ms")
    }
}

pub(super) fn readable_identifier(value: &str) -> String {
    value.replace('_', " ")
}

#[cfg(test)]
mod tests {
    use super::{duration, percentage, readable_identifier};

    #[test]
    fn formats_compact_human_values() {
        assert_eq!(percentage(100_000), "10%");
        assert_eq!(percentage(20_001), "2.0001%");
        assert_eq!(duration(5_000), "5s");
        assert_eq!(duration(1_500), "1500ms");
        assert_eq!(readable_identifier("after_commit"), "after commit");
    }
}
