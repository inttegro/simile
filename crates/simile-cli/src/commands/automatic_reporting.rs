use simile_core::{Context, Error, ReportDiagnostic, ReportDocument, ReportStatus, ops};
use simile_server::{
    RuntimeIncident, RuntimeReportCommand, RuntimeReportError, RuntimeReportSummary,
    RuntimeUpgradeRecommendation,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Connect the HTTP adapter's sanitized incident stream to Simile's reporting operation.
///
/// The server only observes failures. This worker owns consent lookup, deterministic
/// reproduction, upgrade preflight, durable report storage, and submission. Keeping those
/// responsibilities here prevents transport code from learning reporting policy.
pub(super) struct AutomaticReporter {
    sender: Option<mpsc::Sender<RuntimeReportCommand>>,
    worker: Option<JoinHandle<()>>,
}

impl AutomaticReporter {
    pub(super) fn start(ctx: Context, service: &ops::PreparedService) -> Self {
        let (sender, mut receiver) = mpsc::channel::<RuntimeReportCommand>(16);
        let service_name = service.report.service.clone();
        let artifact_bytes = service.artifact_bytes().to_vec();
        let worker = tokio::spawn(async move {
            while let Some(command) = receiver.recv().await {
                match command {
                    RuntimeReportCommand::Automatic(incident) => {
                        process_automatic(
                            ctx.clone(),
                            service_name.clone(),
                            artifact_bytes.clone(),
                            incident,
                        )
                        .await;
                    }
                    RuntimeReportCommand::Prepare {
                        exchange,
                        title,
                        description,
                        reply,
                    } => {
                        let ctx = ctx.clone();
                        let service = service_name.clone();
                        let artifact_bytes = artifact_bytes.clone();
                        let result = tokio::task::spawn_blocking(move || {
                            ops::prepare_runtime_exchange_report(
                                &ctx,
                                ops::RuntimeExchangeReportRequest {
                                    service,
                                    profile: exchange.profile,
                                    artifact_bytes,
                                    operation: exchange.operation,
                                    response_seed: exchange.response_seed,
                                    content_type: exchange.content_type,
                                    body: exchange.body,
                                    parameters: exchange.parameters,
                                    outcome: exchange.outcome,
                                    scenario: exchange.scenario,
                                    fault_profile: exchange.fault_profile,
                                    expected: ops::ExecutedResponse {
                                        status: exchange.expected.status,
                                        content_type: exchange.expected.content_type,
                                        body: exchange.expected.body,
                                    },
                                    title,
                                    description,
                                },
                            )
                            .map(|prepared| report_summary(&prepared.report))
                            .map_err(report_error)
                        })
                        .await
                        .unwrap_or_else(|error| {
                            Err(RuntimeReportError {
                                code: "reporting_unavailable".to_owned(),
                                message: error.to_string(),
                            })
                        });
                        drop(reply.send(result));
                    }
                    RuntimeReportCommand::Submit {
                        report,
                        contact,
                        current_version,
                        reply,
                    } => {
                        let ctx = ctx.clone();
                        let result = tokio::task::spawn_blocking(move || {
                            ops::send_report(
                                &ctx,
                                &ops::SendReportRequest {
                                    report,
                                    current_version,
                                    contact,
                                },
                            )
                            .map(|sent| report_summary(&sent.report))
                            .map_err(report_error)
                        })
                        .await
                        .unwrap_or_else(|error| {
                            Err(RuntimeReportError {
                                code: "reporting_unavailable".to_owned(),
                                message: error.to_string(),
                            })
                        });
                        drop(reply.send(result));
                    }
                }
            }
        });
        Self {
            sender: Some(sender),
            worker: Some(worker),
        }
    }

    pub(super) fn sender(&self) -> Option<mpsc::Sender<RuntimeReportCommand>> {
        self.sender.clone()
    }

    pub(super) async fn finish(mut self) {
        drop(self.sender.take());
        if let Some(worker) = self.worker.take() {
            if let Err(error) = worker.await {
                tracing::warn!(target: "simile::reporting", %error, "automatic reporting stopped unexpectedly");
            }
        }
    }
}

async fn process_automatic(
    ctx: Context,
    service: String,
    artifact_bytes: Vec<u8>,
    incident: RuntimeIncident,
) {
    let operation = incident.operation.clone();
    let result = tokio::task::spawn_blocking(move || {
        ops::process_automatic_report(
            &ctx,
            ops::AutomaticReportRequest {
                service,
                profile: incident.profile,
                artifact_bytes,
                operation: incident.operation,
                seed: incident.seed,
                content_type: incident.content_type,
                body: incident.body,
                parameters: incident.parameters,
                scenario: incident.scenario,
                fault_profile: incident.fault_profile,
                diagnostic: ReportDiagnostic {
                    code: incident.code,
                    message: incident.message,
                    issues: incident.issues,
                },
            },
        )
    })
    .await;

    match result {
        Ok(Ok(Some(report))) if report.sent => tracing::info!(
            target: "simile::reporting",
            report = report.report.id,
            operation,
            "submitted a reproduced emulator issue"
        ),
        Ok(Ok(Some(report))) => tracing::warn!(
            target: "simile::reporting",
            report = report.report.id,
            operation,
            status = ?report.report.status,
            "prepared an emulator issue but did not submit it"
        ),
        Ok(Ok(None)) => {}
        Ok(Err(error)) => tracing::warn!(
            target: "simile::reporting",
            operation,
            %error,
            "could not prepare an automatic emulator issue"
        ),
        Err(error) => tracing::warn!(
            target: "simile::reporting",
            operation,
            %error,
            "automatic reporting worker failed"
        ),
    }
}

fn report_summary(report: &ReportDocument) -> RuntimeReportSummary {
    RuntimeReportSummary {
        id: report.id.clone(),
        title: report.title.clone(),
        status: report.status.display_name().to_owned(),
        created_at: report.created_at.clone(),
        service: report.service.clone(),
        operation: report.operation.clone(),
        artifact_digest: report.artifact_digest.clone(),
        verified: report.verification.reproduced,
        checked_at: report.verification.checked_at.clone(),
        fingerprint: report.verification.fingerprint.clone(),
        submitted: matches!(
            report.status,
            ReportStatus::Submitted
                | ReportStatus::Accepted
                | ReportStatus::NeedsInformation
                | ReportStatus::Resolved
                | ReportStatus::Rejected
        ),
        upgrade_recommendations: report
            .upgrade_recommendations
            .iter()
            .map(|recommendation| RuntimeUpgradeRecommendation {
                component: recommendation.component.clone(),
                current: recommendation.current.clone(),
                available: recommendation.available.clone(),
                command: recommendation.command.clone(),
            })
            .collect(),
    }
}

fn report_error(error: Error) -> RuntimeReportError {
    let code = match &error {
        Error::ReportContactRequired | Error::InvalidReportContact => "report_contact_required",
        Error::ReportingNotDeclared { .. } => "reporting_not_declared",
        Error::InvalidReportId { .. } => "report_not_found",
        Error::ReportDidNotReproduce { .. } => "report_verification_failed",
        _ => "reporting_failed",
    };
    RuntimeReportError {
        code: code.to_owned(),
        message: error.to_string(),
    }
}
