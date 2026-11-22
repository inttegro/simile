use anstyle::{AnsiColor, Effects, Style};
use simile_core::{Context, ops};

use crate::cli::{OutputFormat, ReportingArgs, ReportingChoice};
use crate::commands::Status;
use crate::error::{Error, Result};
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: ReportingArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    if let Some(mode) = arguments.mode {
        if mode == ReportingChoice::Off && arguments.contact.is_some() {
            return Err(Error::InvalidArguments(
                "--contact cannot be used when reporting is off",
            ));
        }
        let report = ops::configure_reporting(
            ctx,
            &ops::ConfigureReportingRequest {
                service: arguments.service,
                profile: arguments.profile,
                automatic: mode == ReportingChoice::Automatic,
                contact: arguments.contact,
            },
        )?;
        match terminal.output() {
            OutputFormat::Json => terminal.json(&report)?,
            OutputFormat::Human => {
                let state = if report.automatic { "automatic" } else { "off" };
                terminal.line(format_args!(
                    "{HEADING}Reporting {state}{HEADING:#} for {} profile {}",
                    report.service, report.profile
                ))?;
                if let Some(endpoint) = report.endpoint {
                    terminal.line(format_args!("{MUTED}destination{MUTED:#} {endpoint}"))?;
                }
                if let Some(contact) = report.contact {
                    terminal.line(format_args!("{MUTED}contact{MUTED:#} {contact}"))?;
                }
            }
        }
        return Ok(Status::Success);
    }

    if arguments.contact.is_some() {
        return Err(Error::InvalidArguments(
            "--contact requires --mode automatic",
        ));
    }
    let preferences = ops::reporting_preferences(ctx, &arguments.service)?;
    match terminal.output() {
        OutputFormat::Json => terminal.json(&preferences)?,
        OutputFormat::Human => {
            terminal.line(format_args!(
                "{HEADING}Reporting{HEADING:#} for {}",
                arguments.service
            ))?;
            if preferences.is_empty() {
                terminal.line(format_args!(
                    "{MUTED}No profile has automatic reporting enabled.{MUTED:#}"
                ))?;
            }
            for preference in preferences {
                terminal.line(format_args!(
                    "{} · {} · {}",
                    preference.profile, preference.contact, preference.endpoint
                ))?;
            }
        }
    }
    Ok(Status::Success)
}
