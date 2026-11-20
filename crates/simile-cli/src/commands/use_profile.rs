use simile_core::{Context, ops};

use crate::cli::UseArgs;
use crate::commands::{Status, profile};
use crate::error::Result;
use crate::terminal::Terminal;

pub fn execute(arguments: UseArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let report = ops::use_profile(ctx, &arguments.service, &arguments.profile)?;
    profile::render(&report, terminal)?;
    Ok(Status::Success)
}
