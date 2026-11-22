use simile_core::{Context, ReportDiagnostic, ops};
use simile_server::RuntimeIncident;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Connect the HTTP adapter's sanitized incident stream to Simile's reporting operation.
///
/// The server only observes failures. This worker owns consent lookup, deterministic
/// reproduction, upgrade preflight, durable report storage, and submission. Keeping those
/// responsibilities here prevents transport code from learning reporting policy.
pub(super) struct AutomaticReporter {
    sender: Option<mpsc::Sender<RuntimeIncident>>,
    worker: Option<JoinHandle<()>>,
}

impl AutomaticReporter {
    pub(super) fn start(ctx: Context, service: &ops::PreparedService) -> Self {
        if service.reporting_endpoint.is_none() {
            return Self {
                sender: None,
                worker: None,
            };
        }

        let (sender, mut receiver) = mpsc::channel::<RuntimeIncident>(16);
        let service_name = service.report.service.clone();
        let artifact_bytes = service.artifact_bytes().to_vec();
        let worker = tokio::spawn(async move {
            while let Some(incident) = receiver.recv().await {
                let ctx = ctx.clone();
                let service = service_name.clone();
                let artifact_bytes = artifact_bytes.clone();
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
                    Ok(Ok(Some(report))) if report.sent => {
                        tracing::info!(
                            target: "simile::reporting",
                            report = report.report.id,
                            operation,
                            "submitted a reproduced emulator issue"
                        );
                    }
                    Ok(Ok(Some(report))) => {
                        tracing::warn!(
                            target: "simile::reporting",
                            report = report.report.id,
                            operation,
                            status = ?report.report.status,
                            "prepared an emulator issue but did not submit it"
                        );
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => {
                        tracing::warn!(
                            target: "simile::reporting",
                            operation,
                            %error,
                            "could not prepare an automatic emulator issue"
                        );
                    }
                    Err(error) => {
                        tracing::warn!(
                            target: "simile::reporting",
                            operation,
                            %error,
                            "automatic reporting worker failed"
                        );
                    }
                }
            }
        });
        Self {
            sender: Some(sender),
            worker: Some(worker),
        }
    }

    pub(super) fn sender(&self) -> Option<mpsc::Sender<RuntimeIncident>> {
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
