use simile_core::{Context, ops};

use crate::cli::TestArgs;
use crate::commands::Status;
use crate::commands::automatic_reporting::AutomaticReporter;
use crate::error::{Error, Result};
use crate::terminal::Terminal;

pub fn execute(arguments: TestArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let ui = terminal.ui();
    ui.banner()?;
    let requested_service = match (&arguments.service, &arguments.config) {
        (Some(service), _) => service.clone(),
        (None, Some(_)) => "service configuration".to_owned(),
        (None, None) => "the current Simfile".to_owned(),
    };
    let preparing = ui.activity(format!("preparing {requested_service}"))?;
    let prepared = ops::prepare_test(
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
    );
    let plan = match prepared {
        Ok(plan) => {
            preparing.complete(format!("prepared {}", plan.service.report.service))?;
            plan
        }
        Err(error) => {
            preparing.fail(format!("could not prepare {requested_service}"))?;
            return Err(error.into());
        }
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(Error::AsyncRuntime)?;
    runtime.block_on(run(ctx.clone(), plan, terminal))
}

async fn run(ctx: Context, plan: ops::TestPlan, terminal: &Terminal) -> Result<Status> {
    let ui = terminal.ui();
    let service = plan.service.report.service.clone();
    let reporting = AutomaticReporter::start(ctx, &plan.service);
    let binding = ui.activity(format!("starting {service}"))?;
    let bound = simile_server::Server::bind_durable_with_options(
        plan.service.report.listen,
        plan.service.artifact,
        plan.state_path,
        simile_server::ServerOptions {
            scenario: plan.scenario,
            fault_profile: plan.fault_profile,
            reporting_sender: reporting.sender(),
        },
    )
    .await;
    let server = match bound {
        Ok(server) => server,
        Err(error) => {
            binding.fail(format!("could not start {service}"))?;
            return Err(error.into());
        }
    };
    let base_url = format!("http://{}", server.local_addr());
    binding.complete(format!("{service} ready at {base_url}"))?;
    let mut environment = plan.environment;
    environment.insert("SIMILE_SERVICE".to_owned(), service.clone());
    environment.insert("SIMILE_BASE_URL".to_owned(), base_url.clone());
    if let Some(profile) = plan.profile.as_deref() {
        ui.note("profile", profile)?;
    }

    let command = ui.activity("running test command")?;
    let command_result = server.run_command(plan.command, environment).await;
    reporting.finish().await;
    let result = match command_result {
        Ok(result) => result,
        Err(error) => {
            command.fail("test command could not run")?;
            return Err(error.into());
        }
    };
    if result.success {
        command.complete("test command passed")?;
        return Ok(Status::Success);
    }
    command.fail("test command failed")?;
    Ok(result
        .exit_code
        .and_then(|code| u8::try_from(code).ok())
        .map_or(Status::Failure, Status::Code))
}
