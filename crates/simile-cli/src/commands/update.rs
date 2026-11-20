use anstyle::{AnsiColor, Effects, Style};
use simile_core::{Context, ops};

use crate::cli::{OutputFormat, UpdateArgs};
use crate::commands::Status;
use crate::error::Result;
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: UpdateArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let report = ops::update(
        ctx,
        &ops::UpdateRequest {
            service: arguments.service,
            config: arguments.config,
        },
    )?;
    render(&report, terminal)?;
    Ok(Status::Success)
}

fn render(report: &ops::RegistrationReport, terminal: &Terminal) -> Result<()> {
    match terminal.output() {
        OutputFormat::Human => {
            terminal.line(format_args!(
                "{HEADING}Updated{HEADING:#} {} {}",
                report.service, report.service_version
            ))?;
            terminal.line(format_args!(
                "{MUTED}listen{MUTED:#} http://{}",
                report.listen
            ))?;
            terminal.line(format_args!(
                "{MUTED}contract{MUTED:#} OpenAPI {} · {} operations",
                report.openapi_version, report.operations
            ))?;
            terminal.line(format_args!(
                "{MUTED}runtime{MUTED:#} {} async operations · {} backends",
                report.async_operations, report.backends
            ))?;
            terminal.line(format_args!(
                "{MUTED}registry{MUTED:#} {}",
                report.directory.display()
            ))?;
            terminal.line(format_args!("{MUTED}revision{MUTED:#} {}", report.revision))
        }
        OutputFormat::Json => terminal.json(report),
    }
}
