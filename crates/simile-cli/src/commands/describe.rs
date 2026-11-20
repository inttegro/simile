mod render;

use simile_core::{Context, ops};

use crate::cli::{DescribeArgs, DescribeTarget};
use crate::commands::Status;
use crate::error::Result;
use crate::terminal::Terminal;

pub fn execute(arguments: DescribeArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    match arguments.target {
        DescribeTarget::Service(arguments) => {
            let report = ops::describe_service(
                ctx,
                &ops::DescribeServiceRequest {
                    service: arguments.service,
                    config: arguments.config,
                    verbose: terminal.is_verbose(),
                },
            )?;
            render::service(&report, terminal)?;
        }
    }
    Ok(Status::Success)
}
