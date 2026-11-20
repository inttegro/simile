use anstyle::{AnsiColor, Effects, Style};
use simile_core::{Context, ops};

use crate::cli::{OutputFormat, ProfilesArgs};
use crate::commands::Status;
use crate::error::Result;
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: ProfilesArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let report = ops::profiles(ctx, &arguments.service)?;
    match terminal.output() {
        OutputFormat::Json => terminal.json(&report)?,
        OutputFormat::Human => {
            terminal.line(format_args!(
                "{HEADING}Profiles{HEADING:#} {} · {} configured",
                report.service,
                report.profiles.len()
            ))?;
            for profile in &report.profiles {
                let current = if profile.current { " · current" } else { "" };
                terminal.line(format_args!(
                    "{MUTED}{}{MUTED:#} {} · {}{current}",
                    profile.status.as_str(),
                    profile.name,
                    profile.principal
                ))?;
            }
        }
    }
    Ok(Status::Success)
}
