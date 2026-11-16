use anstyle::{AnsiColor, Effects, Style};
use simile_core::ops;

use crate::cli::{OutputFormat, RunArgs};
use crate::commands::Status;
use crate::error::{Error, Result};
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: RunArgs, terminal: &Terminal) -> Result<Status> {
    let spec = match (arguments.spec, arguments.spec_url) {
        (Some(path), None) => ops::SpecSource::Path(path),
        (None, Some(url)) => ops::SpecSource::Url(url),
        _ => {
            return Err(Error::InvalidArguments(
                "exactly one of SPEC or --spec-url is required",
            ));
        }
    };
    let operation = match (arguments.operation, arguments.operation_id) {
        (Some(key), None) => ops::OperationSelector::Key(key),
        (None, Some(id)) => ops::OperationSelector::Id(id),
        _ => {
            return Err(Error::InvalidArguments(
                "exactly one of --operation or --operation-id is required",
            ));
        }
    };
    let report = ops::run(&ops::RunRequest {
        spec,
        behaviors: arguments.behavior,
        operation,
        body: arguments.body,
        content_type: arguments.content_type,
        heuristic_policy: arguments.heuristics.into(),
    })?;

    if !terminal.is_verbose() {
        match terminal.output() {
            OutputFormat::Human => terminal.pretty_json(report.response.body.as_ref())?,
            OutputFormat::Json => terminal.json(&report.response.body)?,
        }
        return Ok(Status::Success);
    }

    match terminal.output() {
        OutputFormat::Human => render_human(&report, terminal)?,
        OutputFormat::Json => terminal.json(&report)?,
    }
    Ok(Status::Success)
}

fn render_human(report: &ops::RunReport, terminal: &Terminal) -> Result<()> {
    let operation_id = report
        .operation_id
        .as_deref()
        .map_or_else(String::new, |id| format!(" ({id})"));
    terminal.line(format_args!(
        "{HEADING}Ran{HEADING:#} {}{operation_id}",
        report.operation
    ))?;
    let request_source = match report.request.source {
        ops::RequestSource::None => "none",
        ops::RequestSource::Synthesized => "synthesized",
        ops::RequestSource::Provided => "provided",
    };
    terminal.line(format_args!(
        "{MUTED}request{MUTED:#} {request_source}{}",
        media_suffix(report.request.content_type.as_deref())
    ))?;
    if let Some(body) = &report.request.body {
        terminal.line(format_args!("{body}"))?;
    }
    terminal.line(format_args!(
        "{MUTED}response{MUTED:#} {}{}",
        report.response.status,
        media_suffix(report.response.content_type.as_deref())
    ))?;
    if let Some(body) = &report.response.body {
        terminal.line(format_args!("{body}"))?;
    }
    if report.async_operations.is_empty() {
        terminal.line(format_args!("{MUTED}async{MUTED:#} none"))?;
    } else {
        for operation in &report.async_operations {
            terminal.line(format_args!(
                "{MUTED}async{MUTED:#} {} · {}",
                operation.name,
                operation.resources.join(", ")
            ))?;
        }
    }
    Ok(())
}

fn media_suffix(media_type: Option<&str>) -> String {
    media_type.map_or_else(String::new, |media_type| format!(" · {media_type}"))
}
