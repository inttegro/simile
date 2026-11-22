use simile_core::{Context, ops};

use crate::cli::{OutputFormat, RegisterArgs};
use crate::commands::Status;
use crate::error::Result;
use crate::terminal::Terminal;
use crate::ui::format_count;

pub fn execute(arguments: RegisterArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let ui = terminal.ui();
    ui.banner()?;
    let requested_service = arguments.service.as_deref().unwrap_or("service").to_owned();
    let activity = ui.activity(format!("registering {requested_service}"))?;
    let result = ops::register(
        ctx,
        &ops::RegisterRequest {
            service: arguments.service,
            config: arguments.config,
            force: arguments.force,
        },
    );
    let report = match result {
        Ok(report) => {
            activity.complete(format!("registered {}", report.service))?;
            report
        }
        Err(error) => {
            activity.fail(format!("could not register {requested_service}"))?;
            return Err(error.into());
        }
    };
    render(&report, terminal)?;
    Ok(Status::Success)
}

fn render(report: &ops::RegistrationReport, terminal: &Terminal) -> Result<()> {
    match terminal.output() {
        OutputFormat::Human => {
            let ui = terminal.ui();
            ui.result_title(
                "registered",
                format!("{} {}", report.service, report.service_version),
            )?;
            ui.result_fact("listen", format!("http://{}", report.listen))?;
            ui.result_fact(
                "contract",
                format!(
                    "OpenAPI {} · {}",
                    report.openapi_version,
                    format_count(report.operations, "operation")
                ),
            )?;
            ui.result_fact(
                "runtime",
                format!(
                    "{} · {}",
                    format_count(report.async_operations, "async operation"),
                    format_count(report.backends, "backend")
                ),
            )?;
            ui.result_fact("registry", report.directory.display())?;
            ui.result_fact("revision", &report.revision)
        }
        OutputFormat::Json => terminal.json(report),
    }
}
