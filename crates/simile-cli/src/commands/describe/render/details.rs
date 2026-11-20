use anstyle::{AnsiColor, Effects, Style};
use simile_core::ops;

use super::{percentage, readable_identifier};
use crate::error::Result;
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const LABEL: Style = AnsiColor::Cyan.on_default();
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub(super) fn render(report: &ops::DescribeServiceReport, terminal: &Terminal) -> Result<()> {
    webhooks(report, terminal)?;
    principals(report, terminal)?;
    profiles(report, terminal)?;
    setup_workflows(report, terminal)?;
    operations(report, terminal)?;
    scenarios(report, terminal)?;
    faults(report, terminal)?;
    fault_profiles(report, terminal)?;
    protocol_errors(report, terminal)?;
    backends(report, terminal)?;
    async_operations(report, terminal)?;
    authoring_objects(report, terminal)
}

fn webhooks(report: &ops::DescribeServiceReport, terminal: &Terminal) -> Result<()> {
    if report.webhook_contracts.is_empty() {
        return Ok(());
    }
    section(terminal, "Webhooks", report.webhook_contracts.len())?;
    for webhook in &report.webhook_contracts {
        name(terminal, &webhook.name)?;
        meta(
            terminal,
            format!(
                "{} · {}",
                webhook.method,
                readable_identifier(&webhook.origin)
            ),
        )?;
        description(terminal, webhook.description.as_deref())?;
    }
    Ok(())
}

fn principals(report: &ops::DescribeServiceReport, terminal: &Terminal) -> Result<()> {
    if report.principals.is_empty() {
        return Ok(());
    }
    section(terminal, "Principals", report.principals.len())?;
    for principal in &report.principals {
        name(terminal, &principal.name)?;
        meta(
            terminal,
            format!("security: {}", display_list(&principal.security_schemes)),
        )?;
        description(terminal, principal.description.as_deref())?;
    }
    Ok(())
}

fn profiles(report: &ops::DescribeServiceReport, terminal: &Terminal) -> Result<()> {
    if report.profiles.is_empty() {
        return Ok(());
    }
    section(terminal, "Service profiles", report.profiles.len())?;
    for profile in &report.profiles {
        name(terminal, &profile.name)?;
        meta(
            terminal,
            format!(
                "{} · {} · security: {}",
                profile.principal,
                profile.status,
                display_list(&profile.security_schemes)
            ),
        )?;
        description(terminal, profile.description.as_deref())?;
    }
    Ok(())
}

fn setup_workflows(report: &ops::DescribeServiceReport, terminal: &Terminal) -> Result<()> {
    if report.principal_setups.is_empty() {
        return Ok(());
    }
    section(terminal, "Setup workflows", report.principal_setups.len())?;
    for setup in &report.principal_setups {
        name(terminal, &setup.principal)?;
        meta(terminal, format!("runs {}", setup.operation_id))?;
        description(terminal, setup.description.as_deref())?;
        for collect in &setup.collects {
            terminal.line(format_args!(
                "    {MUTED}collect{MUTED:#} {LABEL}{}{LABEL:#} · {}{}",
                collect.name,
                collect.value_type,
                if collect.required { " · required" } else { "" }
            ))?;
            description_at(terminal, collect.description.as_deref(), 6)?;
        }
        for issue in &setup.issues {
            terminal.line(format_args!(
                "    {MUTED}issue{MUTED:#} {LABEL}{}{LABEL:#} · {}{}{}",
                issue.name,
                issue.source,
                issue
                    .credential
                    .as_deref()
                    .map_or(String::new(), |credential| format!(
                        " · authenticates {credential}"
                    )),
                if issue.sensitive { " · sensitive" } else { "" }
            ))?;
        }
    }
    Ok(())
}

fn operations(report: &ops::DescribeServiceReport, terminal: &Terminal) -> Result<()> {
    if report.operations.is_empty() {
        return Ok(());
    }
    section(terminal, "Operations", report.operations.len())?;
    for operation in &report.operations {
        name(terminal, &operation.key)?;
        meta(
            terminal,
            format!(
                "{}{}",
                readable_identifier(&operation.behavior_source),
                if operation.secured { " · secured" } else { "" }
            ),
        )?;
        description(terminal, operation.description.as_deref())?;
        for outcome in &operation.outcomes {
            terminal.line(format_args!(
                "    {MUTED}outcome{MUTED:#} {LABEL}{}{LABEL:#} · HTTP {}",
                outcome.name, outcome.status
            ))?;
            description_at(terminal, outcome.description.as_deref(), 6)?;
        }
    }
    Ok(())
}

fn scenarios(report: &ops::DescribeServiceReport, terminal: &Terminal) -> Result<()> {
    if report.scenarios.is_empty() {
        return Ok(());
    }
    section(terminal, "Scenarios", report.scenarios.len())?;
    for scenario in &report.scenarios {
        name(terminal, &scenario.name)?;
        meta(
            terminal,
            format!(
                "{} · {}",
                quantity(scenario.selections, "outcome", "outcomes"),
                quantity(scenario.faults, "fault", "faults")
            ),
        )?;
        description(terminal, scenario.description.as_deref())?;
    }
    Ok(())
}

fn faults(report: &ops::DescribeServiceReport, terminal: &Terminal) -> Result<()> {
    if report.faults.is_empty() {
        return Ok(());
    }
    section(terminal, "Faults", report.faults.len())?;
    for fault in &report.faults {
        name(terminal, &fault.name)?;
        meta(
            terminal,
            format!("{} · {}", readable_identifier(&fault.phase), fault.action),
        )?;
        description(terminal, fault.description.as_deref())?;
    }
    Ok(())
}

fn fault_profiles(report: &ops::DescribeServiceReport, terminal: &Terminal) -> Result<()> {
    if report.fault_profiles.is_empty() {
        return Ok(());
    }
    section(terminal, "Fault profiles", report.fault_profiles.len())?;
    for profile in &report.fault_profiles {
        name(terminal, &profile.name)?;
        meta(
            terminal,
            format!(
                "{} · {}",
                percentage(profile.probability_ppm),
                quantity(profile.faults.len(), "fault", "faults")
            ),
        )?;
        description(terminal, profile.description.as_deref())?;
    }
    Ok(())
}

fn protocol_errors(report: &ops::DescribeServiceReport, terminal: &Terminal) -> Result<()> {
    if report.protocol_errors.is_empty() {
        return Ok(());
    }
    section(terminal, "Protocol errors", report.protocol_errors.len())?;
    for error in &report.protocol_errors {
        name(terminal, &error.cause)?;
        meta(
            terminal,
            format!(
                "HTTP {}",
                error
                    .statuses
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )?;
        description(terminal, error.description.as_deref())?;
    }
    Ok(())
}

fn backends(report: &ops::DescribeServiceReport, terminal: &Terminal) -> Result<()> {
    if report.backends.is_empty() {
        return Ok(());
    }
    section(terminal, "Backends", report.backends.len())?;
    for backend in &report.backends {
        name(terminal, &backend.reference)?;
        description(terminal, backend.description.as_deref())?;
    }
    Ok(())
}

fn async_operations(report: &ops::DescribeServiceReport, terminal: &Terminal) -> Result<()> {
    if report.async_operations.is_empty() {
        return Ok(());
    }
    section(terminal, "Async operations", report.async_operations.len())?;
    for operation in &report.async_operations {
        name(terminal, &operation.name)?;
        meta(
            terminal,
            format!("uses {}", display_list(&operation.backends)),
        )?;
        description(terminal, operation.description.as_deref())?;
    }
    Ok(())
}

fn authoring_objects(report: &ops::DescribeServiceReport, terminal: &Terminal) -> Result<()> {
    if report.authoring_objects.is_empty() {
        return Ok(());
    }
    section(
        terminal,
        "Error/outcome docs",
        report.authoring_objects.len(),
    )?;
    for object in &report.authoring_objects {
        let qualified_name = object.parent.as_deref().map_or_else(
            || object.name.clone(),
            |parent| format!("{parent}/{}", object.name),
        );
        name(terminal, &qualified_name)?;
        meta(terminal, readable_identifier(&object.kind))?;
        description(terminal, Some(&object.description))?;
    }
    Ok(())
}

fn section(terminal: &Terminal, title: &str, count: usize) -> Result<()> {
    terminal.line(format_args!(""))?;
    terminal.line(format_args!(
        "{HEADING}{title}{HEADING:#} {MUTED}({count}){MUTED:#}"
    ))
}

fn name(terminal: &Terminal, name: &str) -> Result<()> {
    terminal.line(format_args!("  {LABEL}{name}{LABEL:#}"))
}

fn meta(terminal: &Terminal, value: impl std::fmt::Display) -> Result<()> {
    terminal.line(format_args!("    {MUTED}{value}{MUTED:#}"))
}

fn description(terminal: &Terminal, description: Option<&str>) -> Result<()> {
    description_at(terminal, description, 4)
}

fn description_at(
    terminal: &Terminal,
    description: Option<&str>,
    indentation: usize,
) -> Result<()> {
    if let Some(description) = description {
        terminal.line(format_args!("{}{description}", " ".repeat(indentation)))?;
    }
    Ok(())
}

fn display_list(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_owned()
    } else {
        values.join(", ")
    }
}

fn quantity(value: usize, singular: &str, plural: &str) -> String {
    format!("{value} {}", if value == 1 { singular } else { plural })
}

#[cfg(test)]
mod tests {
    use super::quantity;

    #[test]
    fn pluralizes_inventory_counts() {
        assert_eq!(quantity(1, "fault", "faults"), "1 fault");
        assert_eq!(quantity(0, "fault", "faults"), "0 faults");
    }
}
