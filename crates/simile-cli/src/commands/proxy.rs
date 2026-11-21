use std::path::PathBuf;

use anstyle::{AnsiColor, Effects, Style};
use serde::Serialize;
use simile_core::{Context, ops};
use simile_learning::{LearningReport, Session};

use crate::cli::{OutputFormat, ProxyArgs};
use crate::commands::Status;
use crate::error::{Error, Result};
use crate::terminal::Terminal;

const HEADING: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const MUTED: Style = AnsiColor::BrightBlack.on_default();

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
    let server = simile_server::ProxyServer::bind(
        plan.report.listen,
        simile_server::ProxyOptions {
            upstream: plan.report.upstream.clone(),
            relay_path: plan.relay_path.clone(),
            webhook_destination: plan.report.webhook_destination.clone(),
            learning: learning.clone(),
        },
    )
    .await?;
    plan.report.listen = server.local_addr();
    plan.report.api_base = format!("http://{}", server.local_addr());
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
    terminal.status_line(format_args!(
        "{HEADING}Simile proxy · {}{HEADING:#}",
        report.service
    ))?;
    terminal.status_line(format_args!(
        "{MUTED}listen{MUTED:#} http://{}",
        report.listen
    ))?;
    terminal.status_line(format_args!("{MUTED}upstream{MUTED:#} {}", report.upstream))?;
    terminal.status_line(format_args!("{MUTED}API base{MUTED:#} {}", report.api_base))?;
    if let Some(relay) = &report.webhook_relay {
        terminal.status_line(format_args!("{MUTED}webhook{MUTED:#} {relay}"))?;
        match &report.webhook_destination {
            Some(destination) => {
                terminal.status_line(format_args!("{MUTED}forwards to{MUTED:#} {destination}"))?
            }
            None => terminal.status_line(format_args!(
                "{MUTED}webhook mode{MUTED:#} observe and acknowledge"
            ))?,
        }
    }
    if report.learning {
        terminal.status_line(format_args!("{MUTED}learning{MUTED:#} enabled"))?;
        if let Some(session) = &report.session {
            terminal.status_line(format_args!("{MUTED}session{MUTED:#} {session}"))?;
        }
        if let Some(workspace) = &report.workspace {
            terminal.status_line(format_args!(
                "{MUTED}workspace{MUTED:#} {}",
                workspace.display()
            ))?;
        }
        terminal.status_line(format_args!(
            "{MUTED}privacy{MUTED:#} secrets redacted before recording"
        ))?;
    }
    terminal.status_line(format_args!("Listening. Press Ctrl-C to stop."))
}

fn render_finished(report: &ProxyCommandReport, terminal: &Terminal) -> Result<()> {
    match terminal.output() {
        OutputFormat::Json => terminal.json(report),
        OutputFormat::Human => {
            let Some(learning) = &report.learning else {
                return terminal.line(format_args!("Stopped {} proxy.", report.proxy.service));
            };
            terminal.line(format_args!(
                "{HEADING}Learned {}{HEADING:#}",
                report.proxy.service
            ))?;
            terminal.line(format_args!(
                "{MUTED}traffic{MUTED:#} {} API requests · {} webhook deliveries",
                learning.api_requests, learning.webhook_deliveries
            ))?;
            terminal.line(format_args!(
                "{MUTED}contract{MUTED:#} {} operations · {} response statuses · {} supported bindings",
                learning.operations, learning.statuses, learning.bindings
            ))?;
            terminal.line(format_args!(
                "{MUTED}review{MUTED:#} {} inference gaps",
                learning.unknowns
            ))?;
            terminal.line(format_args!(
                "{MUTED}verified{MUTED:#} generated service compiles"
            ))?;
            terminal.line(format_args!(
                "{MUTED}Simfile{MUTED:#} {}",
                learning.simfile.display()
            ))?;
            terminal.line(format_args!(
                "{MUTED}OpenAPI{MUTED:#} {}",
                learning.openapi.display()
            ))?;
            terminal.line(format_args!(
                "{MUTED}evidence{MUTED:#} {}",
                learning.evidence.display()
            ))
        }
    }
}
