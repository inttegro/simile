use anstyle::{AnsiColor, Effects, Style};
use simile_core::{Context, ops};

use crate::cli::{DescribeArgs, DescribeTarget, OutputFormat};
use crate::commands::Status;
use crate::error::Result;
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const LABEL: Style = AnsiColor::Cyan.on_default();
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: DescribeArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    match arguments.target {
        DescribeTarget::Service(arguments) => {
            let report = ops::describe_service(
                ctx,
                &ops::DescribeServiceRequest {
                    service: arguments.service,
                    config: arguments.config,
                    verbose: terminal.is_verbose(),
                },
            )?;
            render_service(&report, terminal)?;
        }
    }
    Ok(Status::Success)
}

fn render_service(report: &ops::DescribeServiceReport, terminal: &Terminal) -> Result<()> {
    if terminal.output() == OutputFormat::Json {
        return terminal.json(report);
    }

    terminal.line(format_args!(
        "{HEADING}{}{HEADING:#} {}",
        report.service, report.service_version
    ))?;
    if let Some(description) = &report.description {
        terminal.line(format_args!("{description}"))?;
    }
    terminal.line(format_args!(
        "{MUTED}listen{MUTED:#} http://{}",
        report.listen
    ))?;
    terminal.line(format_args!(
        "{MUTED}contract{MUTED:#} OpenAPI {} · {} operations",
        report.openapi_version, report.operation_count
    ))?;
    terminal.line(format_args!("{MUTED}source{MUTED:#} {}", report.source))?;
    if let Some(revision) = &report.revision {
        terminal.line(format_args!("{MUTED}revision{MUTED:#} {revision}"))?;
    }

    if report.operations.is_empty() {
        terminal.line(format_args!(
            "{MUTED}operations{MUTED:#} use --verbose to list every operation and outcome"
        ))?;
    } else {
        terminal.line(format_args!("{HEADING}Operations{HEADING:#}"))?;
        for operation in &report.operations {
            terminal.line(format_args!("{LABEL}{}{LABEL:#}", operation.key))?;
            if let Some(description) = &operation.description {
                terminal.line(format_args!("  {description}"))?;
            }
            for outcome in &operation.outcomes {
                terminal.line(format_args!(
                    "  {MUTED}outcome{MUTED:#} {} · HTTP {}{}",
                    outcome.name,
                    outcome.status,
                    outcome
                        .description
                        .as_deref()
                        .map_or(String::new(), |description| format!(" · {description}"))
                ))?;
            }
        }
    }

    if !report.scenarios.is_empty() {
        terminal.line(format_args!("{HEADING}Scenarios{HEADING:#}"))?;
        for scenario in &report.scenarios {
            terminal.line(format_args!(
                "{LABEL}{}{LABEL:#} · {} outcomes · {} faults{}",
                scenario.name,
                scenario.selections,
                scenario.faults,
                suffix(scenario.description.as_deref())
            ))?;
        }
    }
    if !report.faults.is_empty() {
        terminal.line(format_args!("{HEADING}Faults{HEADING:#}"))?;
        for fault in &report.faults {
            terminal.line(format_args!(
                "{LABEL}{}{LABEL:#} · {} · {}{}",
                fault.name,
                fault.phase,
                fault.action,
                suffix(fault.description.as_deref())
            ))?;
        }
    }
    if !report.fault_profiles.is_empty() {
        terminal.line(format_args!("{HEADING}Fault profiles{HEADING:#}"))?;
        for profile in &report.fault_profiles {
            terminal.line(format_args!(
                "{LABEL}{}{LABEL:#} · {:.4}% · {} faults{}",
                profile.name,
                f64::from(profile.probability_ppm) / 10_000.0,
                profile.faults.len(),
                suffix(profile.description.as_deref())
            ))?;
        }
    }
    if !report.resources.is_empty() {
        terminal.line(format_args!("{HEADING}Resources{HEADING:#}"))?;
        for resource in &report.resources {
            terminal.line(format_args!(
                "{LABEL}{}{LABEL:#}{}",
                resource.reference,
                suffix(resource.description.as_deref())
            ))?;
        }
    }
    if !report.async_operations.is_empty() {
        terminal.line(format_args!("{HEADING}Async operations{HEADING:#}"))?;
        for operation in &report.async_operations {
            terminal.line(format_args!(
                "{LABEL}{}{LABEL:#} · uses {}{}",
                operation.name,
                operation.resources.join(", "),
                suffix(operation.description.as_deref())
            ))?;
        }
    }
    if !report.authoring_objects.is_empty() {
        terminal.line(format_args!("{HEADING}Authoring objects{HEADING:#}"))?;
        for object in &report.authoring_objects {
            let qualified_name = object.parent.as_deref().map_or_else(
                || object.name.clone(),
                |parent| format!("{parent}/{}", object.name),
            );
            terminal.line(format_args!(
                "{LABEL}{}{LABEL:#} {} · {}",
                object.kind, qualified_name, object.description
            ))?;
        }
    }
    Ok(())
}

fn suffix(description: Option<&str>) -> String {
    description.map_or(String::new(), |value| format!(" · {value}"))
}
