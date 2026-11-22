use anstyle::{AnsiColor, Effects, Style};
use simile_core::{Context, ReportStatus, ops};

use crate::cli::{OutputFormat, ReportArgs};
use crate::commands::Status;
use crate::error::{Error, Result};
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();
const WARNING: Style = AnsiColor::Yellow.on_default().effects(Effects::BOLD);

pub fn execute(arguments: ReportArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let mut prepared = if let Some(bundle) = arguments.bundle {
        if arguments.target.starts_with("rpt_") {
            return Err(Error::InvalidArguments(
                "a reproduction bundle must be paired with a service name",
            ));
        }
        let title = arguments.title.ok_or(Error::InvalidArguments(
            "--title is required when preparing a report",
        ))?;
        ops::prepare_report(
            ctx,
            &ops::PrepareReportRequest {
                service: arguments.target,
                profile: arguments.profile,
                bundle,
                title,
                description: arguments.description,
            },
        )?
    } else {
        if !arguments.target.starts_with("rpt_") {
            return Err(Error::InvalidArguments(
                "pass --bundle to prepare a service report, or provide an existing rpt_ identifier",
            ));
        }
        if arguments.title.is_some()
            || arguments.description.is_some()
            || arguments.profile.is_some()
        {
            return Err(Error::InvalidArguments(
                "--title, --description, and --profile apply only when preparing a report",
            ));
        }
        ops::report(ctx, &arguments.target)?
    };

    let sent = if arguments.send {
        let result = ops::send_report(
            ctx,
            &ops::SendReportRequest {
                report: prepared.report.id.clone(),
                current_version: arguments.send_current,
                contact: arguments.contact,
            },
        )?;
        prepared.report = result.report;
        result.sent
    } else {
        false
    };

    match terminal.output() {
        OutputFormat::Json => terminal.json(&prepared)?,
        OutputFormat::Human => render(&prepared, sent, terminal)?,
    }
    Ok(Status::Success)
}

fn render(report: &ops::PreparedReport, sent: bool, terminal: &Terminal) -> Result<()> {
    let verb = if sent { "Sent" } else { "Prepared" };
    terminal.line(format_args!(
        "{HEADING}{verb}{HEADING:#} {}",
        report.report.id
    ))?;
    terminal.line(format_args!(
        "{MUTED}service{MUTED:#} {} {}",
        report.report.service, report.report.service_version
    ))?;
    terminal.line(format_args!(
        "{MUTED}operation{MUTED:#} {}",
        report.report.operation
    ))?;
    terminal.line(format_args!(
        "{MUTED}issue{MUTED:#} {} · {}",
        report.report.diagnostic.code, report.report.diagnostic.message
    ))?;
    terminal.line(format_args!(
        "{MUTED}verified{MUTED:#} {}",
        if report.report.verification.reproduced {
            "yes"
        } else {
            "no"
        }
    ))?;
    if !report.report.upgrade_recommendations.is_empty() {
        terminal.line(format_args!(
            "{WARNING}Upgrade recommended before sending{WARNING:#}"
        ))?;
        for recommendation in &report.report.upgrade_recommendations {
            terminal.line(format_args!(
                "  {} {} → {} · {}",
                recommendation.component,
                recommendation.current,
                recommendation.available,
                recommendation.command
            ))?;
        }
    }
    terminal.line(format_args!(
        "{MUTED}status{MUTED:#} {}",
        report.report.status.display_name()
    ))?;
    terminal.line(format_args!(
        "{MUTED}review{MUTED:#} {}",
        report.directory.display()
    ))?;
    if !sent && report.report.status == ReportStatus::Verified {
        terminal.line(format_args!(
            "{MUTED}send{MUTED:#} simile report {} --send",
            report.report.id
        ))?;
    }
    Ok(())
}
