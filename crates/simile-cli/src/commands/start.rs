use anstyle::{AnsiColor, Effects, Style};
use simile_core::{Context, ops};

use crate::cli::{OutputFormat, StartArgs};
use crate::commands::Status;
use crate::error::{Error, Result};
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

pub fn execute(arguments: StartArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let scenario = arguments.scenario;
    let fault_profile = arguments.fault_profile;
    let prepared = ops::prepare_service(
        ctx,
        &ops::StartRequest {
            service: arguments.service,
            config: arguments.config,
            port: arguments.port,
        },
    )?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(Error::AsyncRuntime)?;
    runtime.block_on(run(prepared, scenario, fault_profile, terminal))
}

async fn run(
    mut prepared: ops::PreparedService,
    scenario: Option<String>,
    fault_profile: Option<String>,
    terminal: &Terminal,
) -> Result<Status> {
    let server = simile_server::Server::bind_with_options(
        prepared.report.listen,
        prepared.artifact,
        simile_server::ServerOptions {
            scenario,
            fault_profile: fault_profile.clone(),
        },
    )
    .await?;
    prepared.report.listen = server.local_addr();
    prepared.report.active_fault_profile = fault_profile;
    prepared.report.fault_seed = server.fault_seed();
    render(&prepared.report, terminal)?;
    server.run().await?;
    Ok(Status::Success)
}

fn render(report: &ops::ServiceReport, terminal: &Terminal) -> Result<()> {
    match terminal.output() {
        OutputFormat::Human => {
            terminal.line(format_args!(
                "{HEADING}Started{HEADING:#} {} {}",
                report.service, report.service_version
            ))?;
            terminal.line(format_args!(
                "{MUTED}listen{MUTED:#} http://{}",
                report.listen
            ))?;
            terminal.line(format_args!(
                "{MUTED}contract{MUTED:#} OpenAPI {} · {} operations",
                report.openapi_version, report.operations
            ))?;
            terminal.line(format_args!(
                "{MUTED}runtime{MUTED:#} {} async operations · {} resources",
                report.async_operations, report.resources
            ))?;
            terminal.line(format_args!(
                "{MUTED}faults{MUTED:#} {} definitions · {} profiles",
                report.faults, report.fault_profiles
            ))?;
            if let (Some(profile), Some(seed)) =
                (report.active_fault_profile.as_deref(), report.fault_seed)
            {
                terminal.line(format_args!(
                    "{MUTED}faults{MUTED:#} {profile} · seed {seed}"
                ))?;
            }
            Ok(())
        }
        OutputFormat::Json => terminal.json(report),
    }
}
