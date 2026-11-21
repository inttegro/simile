use anstyle::{AnsiColor, Effects, Style};
use simile_core::{Context, ops};

use crate::cli::TestArgs;
use crate::commands::Status;
use crate::error::{Error, Result};
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: TestArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let plan = ops::prepare_test(
        ctx,
        ops::TestRequest {
            service: arguments.service,
            config: arguments.config,
            profile: arguments.profile,
            port: arguments.port,
            scenario: arguments.scenario,
            fault_profile: arguments.fault_profile,
            command: arguments.command,
        },
    )?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(Error::AsyncRuntime)?;
    runtime.block_on(run(plan, terminal))
}

async fn run(plan: ops::TestPlan, terminal: &Terminal) -> Result<Status> {
    let service = plan.service.report.service.clone();
    let server = simile_server::Server::bind_durable_with_options(
        plan.service.report.listen,
        plan.service.artifact,
        plan.state_path,
        simile_server::ServerOptions {
            scenario: plan.scenario,
            fault_profile: plan.fault_profile,
        },
    )
    .await?;
    let base_url = format!("http://{}", server.local_addr());
    let mut environment = plan.environment;
    environment.insert("SIMILE_SERVICE".to_owned(), service.clone());
    environment.insert("SIMILE_BASE_URL".to_owned(), base_url.clone());
    terminal.status_line(format_args!(
        "{HEADING}Testing{HEADING:#} {} {MUTED}at{MUTED:#} {}",
        service, base_url
    ))?;
    if let Some(profile) = plan.profile.as_deref() {
        terminal.status_line(format_args!("{MUTED}profile{MUTED:#} {profile}"))?;
    }

    let result = server.run_command(plan.command, environment).await?;
    if result.success {
        return Ok(Status::Success);
    }
    Ok(result
        .exit_code
        .and_then(|code| u8::try_from(code).ok())
        .map_or(Status::Failure, Status::Code))
}
