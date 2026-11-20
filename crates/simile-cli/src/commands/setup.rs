use std::collections::BTreeMap;
use std::io::{BufRead as _, IsTerminal as _, Write as _};

use anstyle::{AnsiColor, Effects, Style};
use serde_json::Value;
use simile_core::{Context, ops};
use simile_spec::{SetupCollect, SetupValueType};

use crate::cli::{OutputFormat, SetupArgs};
use crate::commands::Status;
use crate::error::{Error, Result};
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: SetupArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let contract = ops::setup_contract(ctx, &arguments.service, arguments.principal.as_deref())?;
    let mut values = parse_values(&arguments.values, &contract.collects)?;
    if std::io::stdin().is_terminal() {
        prompt_missing(&contract.collects, &mut values)?;
    }
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
    match terminal.output() {
        OutputFormat::Json => terminal.json(&report)?,
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
        }
    }
    Ok(Status::Success)
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
