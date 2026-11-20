use anstyle::{AnsiColor, Effects, Style};
use simile_core::{Context, ops};

use crate::cli::{OutputFormat, ProfileArgs};
use crate::commands::Status;
use crate::error::Result;
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: ProfileArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let report = ops::profile(ctx, &arguments.service, &arguments.profile)?;
    render(&report, terminal)?;
    Ok(Status::Success)
}

pub(crate) fn render(report: &ops::ProfileReport, terminal: &Terminal) -> Result<()> {
    match terminal.output() {
        OutputFormat::Json => terminal.json(report),
        OutputFormat::Human => {
            terminal.line(format_args!(
                "{HEADING}Profile{HEADING:#} {} · {}",
                report.service, report.profile.name
            ))?;
            terminal.line(format_args!(
                "{MUTED}status{MUTED:#} {}",
                report.profile.status.as_str()
            ))?;
            terminal.line(format_args!(
                "{MUTED}principal{MUTED:#} {}",
                report.profile.principal
            ))?;
            terminal.line(format_args!(
                "{MUTED}current{MUTED:#} {}",
                if report.profile.current { "yes" } else { "no" }
            ))?;
            terminal.line(format_args!(
                "{MUTED}credentials{MUTED:#} {}",
                report.credentials_file.display()
            ))?;
            let schemes = if report.profile.credential_schemes.is_empty() {
                "none".to_owned()
            } else {
                report.profile.credential_schemes.join(", ")
            };
            terminal.line(format_args!("{MUTED}authentication{MUTED:#} {schemes}"))
        }
    }
}
