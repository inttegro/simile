use anstyle::{AnsiColor, Effects, Style};
use simile_core::{Context, ops};

use crate::cli::{OutputFormat, StateArgs};
use crate::commands::Status;
use crate::error::{Error, Result};
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: StateArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let report = ops::inspect_state(
        ctx,
        &ops::StateRequest {
            service: arguments.service,
            resource: arguments.resource,
            causation_id: arguments.causation_id,
            correlation_id: arguments.correlation_id,
            after_sequence: arguments.after,
            limit: arguments.limit,
            include_values: arguments.values,
        },
    )?;
    match terminal.output() {
        OutputFormat::Json => terminal.json(&report)?,
        OutputFormat::Human => {
            terminal.line(format_args!(
                "{HEADING}State{HEADING:#} {} · {} resources · {} events",
                report.service,
                report.resources.len(),
                report.total_events
            ))?;
            for resource in &report.resources {
                terminal.line(format_args!("{MUTED}resource{MUTED:#} {resource}"))?;
            }
            for event in &report.events {
                terminal.line(format_args!(
                    "{MUTED}#{sequence}{MUTED:#} {resource} {action} {outcome:?}",
                    sequence = event.sequence,
                    resource = event.resource,
                    action = event.action,
                    outcome = event.outcome
                ))?;
            }
            for (resource, value) in &report.values {
                terminal.line(format_args!("{HEADING}{resource}{HEADING:#}"))?;
                let encoded = serde_json::to_string_pretty(value).map_err(Error::Serialize)?;
                terminal.line(format_args!("{encoded}"))?;
            }
        }
    }
    Ok(Status::Success)
}
