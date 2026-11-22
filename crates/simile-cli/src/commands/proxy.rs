use std::path::PathBuf;

use serde::Serialize;
use simile_core::{Context, ops};
use simile_learning::{LearningReport, Session};

use crate::cli::{OutputFormat, ProxyArgs};
use crate::commands::Status;
use crate::error::{Error, Result};
use crate::terminal::Terminal;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProxyCommandReport {
    proxy: ops::ProxyReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    learning: Option<LearningReport>,
    contract_verified: bool,
}

pub fn execute(arguments: ProxyArgs, ctx: &Context, terminal: &Terminal) -> Result<Status> {
    let plan = ops::prepare_proxy(
        ctx,
        &ops::ProxyRequest {
            service: arguments.service,
            upstream: arguments.upstream,
            learn: arguments.learn,
            public_url: arguments.public_url,
            webhook_destination: arguments.webhook_destination,
            host: arguments.host,
            port: arguments.port,
        },
    )?;
    let learning = learning_session(&plan)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(Error::AsyncRuntime)?;
    runtime.block_on(run(plan, learning, ctx, terminal))
}

fn learning_session(plan: &ops::ProxyPlan) -> Result<Option<Session>> {
    match (
        plan.report.session.as_deref(),
        plan.learning_workspace.as_ref(),
    ) {
        (Some(session), Some(workspace)) => Ok(Some(Session::create(
            &plan.report.service,
            session,
            workspace,
        )?)),
        (None, None) => Ok(None),
        _ => Err(crate::error::Error::InvalidArguments(
            "learning session is incomplete",
        )),
    }
}

async fn run(
    mut plan: ops::ProxyPlan,
    learning: Option<Session>,
    ctx: &Context,
    terminal: &Terminal,
) -> Result<Status> {
    let ui = terminal.ui();
    ui.banner()?;
    let binding = ui.activity(format!("binding {} proxy", plan.report.service))?;
    let bound = simile_server::ProxyServer::bind(
        plan.report.listen,
        simile_server::ProxyOptions {
            upstream: plan.report.upstream.clone(),
            relay_path: plan.relay_path.clone(),
            webhook_destination: plan.report.webhook_destination.clone(),
            learning: learning.clone(),
        },
    )
    .await;
    let server = match bound {
        Ok(server) => server,
        Err(error) => {
            binding.fail(format!("could not bind {} proxy", plan.report.service))?;
            return Err(error.into());
        }
    };
    plan.report.listen = server.local_addr();
    plan.report.api_base = format!("http://{}", server.local_addr());
    binding.complete_timeline(format!(
        "{} proxy ready at {}",
        plan.report.service, plan.report.api_base
    ))?;
    render_started(&plan.report, terminal)?;
    server.run().await?;

    let learning = learning
        .map(|session| session.finish(&plan.report.upstream, plan.report.listen.port()))
        .transpose()?;
    let contract_verified = if let Some(report) = &learning {
        verify_generated_service(ctx, &plan.report.service, report.simfile.clone())?;
        true
    } else {
        false
    };
    let report = ProxyCommandReport {
        proxy: plan.report,
        learning,
        contract_verified,
    };
    render_finished(&report, terminal)?;
    Ok(Status::Success)
}

fn verify_generated_service(ctx: &Context, service: &str, simfile: PathBuf) -> Result<()> {
    let _ = ops::prepare_service(
        ctx,
        &ops::StartRequest {
            service: Some(service.to_owned()),
            config: Some(simfile),
            host: None,
            port: None,
        },
    )?;
    Ok(())
}

fn render_started(report: &ops::ProxyReport, terminal: &Terminal) -> Result<()> {
    let ui = terminal.ui();
    ui.note("listen", format!("http://{}", report.listen))?;
    ui.note("upstream", &report.upstream)?;
    ui.note("API base", &report.api_base)?;
    if let Some(relay) = &report.webhook_relay {
        ui.note("webhook", relay)?;
        match &report.webhook_destination {
            Some(destination) => ui.note("forwards to", destination)?,
            None => ui.note("webhook mode", "observe and acknowledge")?,
        }
    }
    if report.learning {
        ui.note("learning", "enabled")?;
        if let Some(session) = &report.session {
            ui.note("session", session)?;
        }
        if let Some(workspace) = &report.workspace {
            ui.note("workspace", workspace.display())?;
        }
        ui.note("privacy", "secrets redacted before recording")?;
    }
    ui.note("status", "listening · press Ctrl-C to stop")
}

fn render_finished(report: &ProxyCommandReport, terminal: &Terminal) -> Result<()> {
    match terminal.output() {
        OutputFormat::Json => terminal.json(report),
        OutputFormat::Human => {
            let Some(learning) = &report.learning else {
                return terminal.line(format_args!("Stopped {} proxy.", report.proxy.service));
            };
            let ui = terminal.ui();
            ui.result_title("learned", &report.proxy.service)?;
            ui.result_fact(
                "traffic",
                format!(
                    "{} API requests · {} webhook deliveries",
                    learning.api_requests, learning.webhook_deliveries
                ),
            )?;
            ui.result_fact(
                "contract",
                format!(
                    "{} operations · {} response statuses · {} supported bindings",
                    learning.operations, learning.statuses, learning.bindings
                ),
            )?;
            ui.result_fact("review", format!("{} inference gaps", learning.unknowns))?;
            ui.result_fact("verified", "generated service compiles")?;
            ui.result_fact("Simfile", learning.simfile.display())?;
            ui.result_fact("OpenAPI", learning.openapi.display())?;
            ui.result_fact("evidence", learning.evidence.display())
        }
    }
}
