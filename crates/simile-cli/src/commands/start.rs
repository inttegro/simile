use simile_core::{Context, ops};
use std::path::PathBuf;

use crate::cli::{OutputFormat, StartArgs};
use crate::commands::Status;
use crate::commands::automatic_reporting::AutomaticReporter;
use crate::error::{Error, Result};
use crate::terminal::Terminal;
use crate::ui::{TableRow, format_count};

pub fn execute(arguments: StartArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let ui = terminal.ui();
    ui.banner()?;
    let scenario = arguments.scenario;
    let fault_profile = arguments.fault_profile;
    let requested_service = match (&arguments.service, &arguments.config) {
        (Some(service), _) => service.clone(),
        (None, Some(_)) => "service configuration".to_owned(),
        (None, None) => "the current Simfile".to_owned(),
    };
    let preparing = ui.activity(format!("preparing {requested_service}"))?;
    let result = ops::prepare_service(
        ctx,
        &ops::StartRequest {
            service: arguments.service,
            config: arguments.config,
            host: arguments.host,
            port: arguments.port,
        },
    );
    let prepared = match result {
        Ok(prepared) => {
            preparing.complete(format!("prepared {}", prepared.report.service))?;
            prepared
        }
        Err(error) => {
            preparing.fail(format!("could not prepare {requested_service}"))?;
            return Err(error.into());
        }
    };
    let state_path = ops::service_state_path(ctx, &prepared.report.service)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(Error::AsyncRuntime)?;
    runtime.block_on(run(
        ctx.clone(),
        prepared,
        scenario,
        fault_profile,
        state_path,
        terminal,
    ))
}

async fn run(
    ctx: Context,
    mut prepared: ops::PreparedService,
    scenario: Option<String>,
    fault_profile: Option<String>,
    state_path: PathBuf,
    terminal: &Terminal,
) -> Result<Status> {
    let ui = terminal.ui();
    let reporting = AutomaticReporter::start(ctx, &prepared);
    let binding = ui.activity(format!("binding {}", prepared.report.service))?;
    let bound = simile_server::Server::bind_durable_with_options(
        prepared.report.listen,
        prepared.artifact,
        state_path,
        simile_server::ServerOptions {
            scenario,
            fault_profile: fault_profile.clone(),
            incident_sender: reporting.sender(),
        },
    )
    .await;
    let server = match bound {
        Ok(server) => server,
        Err(error) => {
            binding.fail(format!("could not bind {}", prepared.report.service))?;
            return Err(error.into());
        }
    };
    prepared.report.listen = server.local_addr();
    prepared.report.active_fault_profile = fault_profile;
    prepared.report.fault_seed = server.fault_seed();
    binding.complete_timeline(format!(
        "{} ready at http://{}",
        prepared.report.service, prepared.report.listen
    ))?;
    render(&prepared.report, terminal)?;
    let result = server.run().await;
    reporting.finish().await;
    result?;
    Ok(Status::Success)
}

fn render(report: &ops::ServiceReport, terminal: &Terminal) -> Result<()> {
    match terminal.output() {
        OutputFormat::Human => {
            let ui = terminal.ui();
            ui.result_title(
                "serving",
                format!("{} {}", report.service, report.service_version),
            )?;
            let mut rows = vec![
                TableRow::new("service", &report.service, &report.service_version),
                TableRow::new(
                    "listen",
                    report.listen.ip().to_string(),
                    format!("port {}", report.listen.port()),
                ),
                TableRow::new(
                    "contract",
                    format!("OpenAPI {}", report.openapi_version),
                    format_count(report.operations, "operation"),
                ),
                TableRow::new(
                    "runtime",
                    format_count(report.async_operations, "async operation"),
                    format_count(report.backends, "backend"),
                ),
                TableRow::new(
                    "faults",
                    format_count(report.faults, "definition"),
                    format_count(report.fault_profiles, "profile"),
                ),
            ];
            if let (Some(profile), Some(seed)) =
                (report.active_fault_profile.as_deref(), report.fault_seed)
            {
                rows.push(TableRow::new(
                    "fault profile",
                    profile,
                    format!("seed {seed}"),
                ));
            }
            ui.table("Manifest", &rows)
        }
        OutputFormat::Json => terminal.json(report),
    }
}
