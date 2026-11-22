use std::collections::BTreeMap;
use std::io::{BufRead as _, IsTerminal as _, Write as _};

use anstyle::{AnsiColor, Effects, Style};
use serde::Serialize;
use serde_json::Value;
use simile_core::{Context, ops};
use simile_spec::{SetupCollect, SetupValueType};

use crate::cli::{OutputFormat, ReportingChoice, SetupArgs};
use crate::commands::Status;
use crate::error::{Error, Result};
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: SetupArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let reporting_destination = ops::reporting_destination(ctx, &arguments.service)?;
    let contract = ops::setup_contract(ctx, &arguments.service, arguments.principal.as_deref())?;
    let mut values = parse_values(&arguments.values, &contract.collects)?;
    if std::io::stdin().is_terminal() {
        prompt_missing(&contract.collects, &mut values)?;
    }
    let reporting = reporting_decision(
        arguments.reporting,
        arguments.report_contact,
        reporting_destination.as_deref(),
        terminal,
    )?;
    let report = ops::setup_profile(
        ctx,
        &ops::SetupProfileRequest {
            service: arguments.service,
            principal: arguments.principal,
            profile: arguments.profile,
            values,
            select: arguments.select,
        },
    )?;
    let reporting_report = reporting
        .map(|decision| {
            ops::configure_reporting(
                ctx,
                &ops::ConfigureReportingRequest {
                    service: report.service.clone(),
                    profile: Some(report.profile.name.clone()),
                    automatic: decision.mode == ReportingChoice::Automatic,
                    contact: decision.contact,
                },
            )
        })
        .transpose()?;
    match terminal.output() {
        OutputFormat::Json => terminal.json(&SetupOutput {
            profile: &report,
            reporting: reporting_report.as_ref(),
        })?,
        OutputFormat::Human => {
            let verb = if report.created { "Created" } else { "Updated" };
            terminal.line(format_args!(
                "{HEADING}{verb}{HEADING:#} {} profile {} for {}",
                report.profile.principal, report.profile.name, report.service
            ))?;
            terminal.line(format_args!(
                "{MUTED}credentials{MUTED:#} {}",
                report.credentials_file.display()
            ))?;
            terminal.line(format_args!(
                "{MUTED}current{MUTED:#} {}",
                if report.profile.current { "yes" } else { "no" }
            ))?;
            if !report.profile.issued.is_empty() {
                let issued = report
                    .profile
                    .issued
                    .iter()
                    .map(|field| {
                        if field.sensitive {
                            format!("{} (sensitive)", field.name)
                        } else {
                            field.name.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                terminal.line(format_args!("{MUTED}issued{MUTED:#} {issued}"))?;
            }
            if let Some(reporting) = reporting_report.as_ref() {
                terminal.line(format_args!(
                    "{MUTED}reporting{MUTED:#} {}",
                    if reporting.automatic {
                        "automatic"
                    } else {
                        "off"
                    }
                ))?;
                if let Some(contact) = reporting.contact.as_deref() {
                    terminal.line(format_args!("{MUTED}contact{MUTED:#} {contact}"))?;
                }
            }
        }
    }
    Ok(Status::Success)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SetupOutput<'a> {
    #[serde(flatten)]
    profile: &'a ops::SetupProfileReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    reporting: Option<&'a ops::ConfigureReportingReport>,
}

struct ReportingDecision {
    mode: ReportingChoice,
    contact: Option<String>,
}

fn reporting_decision(
    mode: Option<ReportingChoice>,
    contact: Option<String>,
    destination: Option<&str>,
    terminal: &Terminal,
) -> Result<Option<ReportingDecision>> {
    if mode == Some(ReportingChoice::Off) && contact.is_some() {
        return Err(Error::InvalidArguments(
            "--report-contact cannot be used when reporting is off",
        ));
    }
    if let Some(mode) = mode {
        if mode == ReportingChoice::Automatic && destination.is_none() {
            return Err(Error::InvalidArguments(
                "this service does not declare a reporting destination",
            ));
        }
        if mode == ReportingChoice::Automatic
            && contact.is_none()
            && !std::io::stdin().is_terminal()
        {
            return Err(Error::InvalidArguments(
                "--report-contact is required for non-interactive automatic reporting setup",
            ));
        }
        let contact = if mode == ReportingChoice::Automatic && contact.is_none() {
            Some(prompt_contact()?)
        } else {
            contact
        };
        return Ok(Some(ReportingDecision { mode, contact }));
    }
    if contact.is_some() {
        return Err(Error::InvalidArguments(
            "--report-contact requires --reporting automatic",
        ));
    }
    if terminal.output() != OutputFormat::Human
        || !std::io::stdin().is_terminal()
        || destination.is_none()
    {
        return Ok(None);
    }
    prompt_reporting(destination.unwrap_or_default())
}

fn prompt_reporting(destination: &str) -> Result<Option<ReportingDecision>> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stderr = std::io::stderr();
    let mut output = stderr.lock();
    writeln!(output, "\nAutomatic issue reporting").map_err(Error::Prompt)?;
    writeln!(
        output,
        "Simile sends a sanitized report only after reproducing an internal emulator failure."
    )
    .map_err(Error::Prompt)?;
    writeln!(output, "Destination: {destination}").map_err(Error::Prompt)?;
    write!(output, "Enable automatic reporting? [y/N]: ").map_err(Error::Prompt)?;
    output.flush().map_err(Error::Prompt)?;
    let mut line = String::new();
    input.read_line(&mut line).map_err(Error::Prompt)?;
    if !matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        return Ok(None);
    }
    write!(
        output,
        "Contact email for questions and fix notifications: "
    )
    .map_err(Error::Prompt)?;
    output.flush().map_err(Error::Prompt)?;
    line.clear();
    input.read_line(&mut line).map_err(Error::Prompt)?;
    let contact = line.trim();
    if contact.is_empty() {
        return Err(Error::InvalidArguments(
            "a contact email is required for automatic reporting",
        ));
    }
    Ok(Some(ReportingDecision {
        mode: ReportingChoice::Automatic,
        contact: Some(contact.to_owned()),
    }))
}

fn prompt_contact() -> Result<String> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stderr = std::io::stderr();
    let mut output = stderr.lock();
    write!(
        output,
        "Contact email for questions and fix notifications: "
    )
    .map_err(Error::Prompt)?;
    output.flush().map_err(Error::Prompt)?;
    let mut line = String::new();
    input.read_line(&mut line).map_err(Error::Prompt)?;
    let contact = line.trim();
    if contact.is_empty() {
        return Err(Error::InvalidArguments(
            "a contact email is required for automatic reporting",
        ));
    }
    Ok(contact.to_owned())
}

fn parse_values(
    arguments: &[String],
    fields: &BTreeMap<String, SetupCollect>,
) -> Result<BTreeMap<String, Value>> {
    let mut values = BTreeMap::new();
    for argument in arguments {
        let (name, raw) = argument
            .split_once('=')
            .ok_or_else(|| Error::InvalidSetupAssignment(argument.clone()))?;
        let field = fields
            .get(name)
            .ok_or_else(|| Error::UnknownSetupField(name.to_owned()))?;
        let value = parse_value(raw, field)?;
        if values.insert(name.to_owned(), value).is_some() {
            return Err(Error::DuplicateSetupField(name.to_owned()));
        }
    }
    Ok(values)
}

fn prompt_missing(
    fields: &BTreeMap<String, SetupCollect>,
    values: &mut BTreeMap<String, Value>,
) -> Result<()> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stderr = std::io::stderr();
    let mut output = stderr.lock();
    let missing = fields
        .values()
        .filter(|field| !values.contains_key(&field.name))
        .cloned()
        .collect::<Vec<_>>();
    for field in &missing {
        let description = field
            .description
            .as_deref()
            .map_or_else(String::new, |description| format!(" — {description}"));
        let optional = if field.required { "" } else { " (optional)" };
        write!(output, "{}{}{}: ", field.name, optional, description).map_err(Error::Prompt)?;
        output.flush().map_err(Error::Prompt)?;
        let mut line = String::new();
        input.read_line(&mut line).map_err(Error::Prompt)?;
        let raw = line.trim();
        if raw.is_empty() && !field.required {
            continue;
        }
        values.insert(field.name.clone(), parse_value(raw, field)?);
    }
    Ok(())
}

fn parse_value(raw: &str, field: &SetupCollect) -> Result<Value> {
    match field.value_type {
        SetupValueType::String | SetupValueType::Url => Ok(Value::String(raw.to_owned())),
        SetupValueType::Boolean => {
            raw.parse::<bool>()
                .map(Value::Bool)
                .map_err(|_| Error::InvalidSetupValue {
                    name: field.name.clone(),
                    expected: "true or false",
                })
        }
        SetupValueType::Integer => {
            raw.parse::<i64>()
                .map(Value::from)
                .map_err(|_| Error::InvalidSetupValue {
                    name: field.name.clone(),
                    expected: "an integer",
                })
        }
        SetupValueType::Json => serde_json::from_str(raw).map_err(|_| Error::InvalidSetupValue {
            name: field.name.clone(),
            expected: "valid JSON",
        }),
    }
}
