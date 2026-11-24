pub(crate) mod terminal;
pub(crate) mod web;

use std::net::SocketAddr;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use simile_core::{Context, ops};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::commands::automatic_reporting::AutomaticReporter;
use crate::error::{Error, Result};

pub(crate) struct ConsoleSession {
    ctx: Context,
    catalog: ops::ConsoleCatalog,
    runtime: Option<SelectedRuntime>,
}

struct SelectedRuntime {
    service: String,
    address: SocketAddr,
    ownership: RuntimeOwnership,
}

enum RuntimeOwnership {
    Attached,
    Owned {
        shutdown: oneshot::Sender<()>,
        task: JoinHandle<simile_server::Result<()>>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InspectionIdentity {
    service: String,
    service_version: String,
    artifact_digest: Option<String>,
    operations: usize,
}

enum Probe {
    Unreachable,
    Service(InspectionIdentity),
    Other,
}

impl ConsoleSession {
    pub(crate) async fn open(ctx: Context, service: Option<String>) -> Result<Self> {
        let catalog = ops::console(&ctx, &ops::ConsoleRequest { service })?;
        let selected = catalog
            .selected
            .as_ref()
            .ok_or(Error::NoRegisteredServices)?;
        let service = selected.service.name.clone();
        let mut session = Self {
            ctx,
            catalog,
            runtime: None,
        };
        session.activate(&service).await?;
        Ok(session)
    }

    pub(crate) fn catalog(&self) -> &ops::ConsoleCatalog {
        &self.catalog
    }

    pub(crate) fn selected(&self) -> &ops::ConsoleService {
        self.catalog
            .selected
            .as_ref()
            .unwrap_or_else(|| unreachable!("open console always has a selected service"))
    }

    pub(crate) fn reports(&self) -> Result<ops::ReportsReport> {
        Ok(ops::reports(
            &self.ctx,
            Some(&self.selected().service.name),
        )?)
    }

    pub(crate) fn runtime_address(&self) -> SocketAddr {
        self.runtime
            .as_ref()
            .map(|runtime| runtime.address)
            .unwrap_or_else(|| self.selected().service.listen)
    }

    pub(crate) fn owns_runtime(&self) -> bool {
        self.runtime
            .as_ref()
            .is_some_and(|runtime| matches!(runtime.ownership, RuntimeOwnership::Owned { .. }))
    }

    pub(crate) async fn select(&mut self, service: &str) -> Result<()> {
        if self
            .catalog
            .selected
            .as_ref()
            .is_some_and(|selected| selected.service.name == service)
        {
            return Ok(());
        }
        let next = ops::console(
            &self.ctx,
            &ops::ConsoleRequest {
                service: Some(service.to_owned()),
            },
        )?;
        let previous = self.selected().service.name.clone();
        self.stop_owned_runtime().await;
        self.catalog = next;
        if let Err(error) = self.activate(service).await {
            self.catalog = ops::console(
                &self.ctx,
                &ops::ConsoleRequest {
                    service: Some(previous.clone()),
                },
            )?;
            if let Err(restore_error) = self.activate(&previous).await {
                tracing::warn!(%restore_error, %previous, "could not restore the previous console service");
            }
            return Err(error);
        }
        Ok(())
    }

    pub(crate) async fn inspect(&self, path: &str) -> Option<Value> {
        let response = raw_request(self.runtime_address(), "GET", path, None)
            .await
            .ok()?;
        if response.status != 200 {
            return None;
        }
        serde_json::from_slice(&response.body).ok()
    }

    pub(crate) async fn act(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value> {
        let encoded = body
            .map(serde_json::to_vec)
            .transpose()
            .map_err(Error::Serialize)?;
        let response = raw_request(self.runtime_address(), method, path, encoded.as_deref())
            .await
            .map_err(Error::ConsoleRequest)?;
        let decoded = serde_json::from_slice::<Value>(&response.body).map_err(Error::Serialize)?;
        if (200..300).contains(&response.status) {
            return Ok(decoded);
        }
        let message = decoded
            .pointer("/error/message")
            .or_else(|| decoded.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("runtime inspection action failed");
        Err(Error::ConsoleAction(format!(
            "{message} (HTTP {})",
            response.status
        )))
    }

    pub(crate) async fn close(&mut self) {
        self.stop_owned_runtime().await;
    }

    async fn activate(&mut self, service: &str) -> Result<()> {
        let selected = self.selected();
        let address = selected.service.listen;
        let ownership = match probe(address).await {
            Probe::Service(identity) if inspection_matches(&identity, service, selected) => {
                RuntimeOwnership::Attached
            }
            Probe::Service(_) | Probe::Other => {
                return Err(Error::ConsoleServiceConflict {
                    service: service.to_owned(),
                    address,
                });
            }
            Probe::Unreachable => {
                let prepared = ops::prepare_service(
                    &self.ctx,
                    &ops::StartRequest {
                        service: Some(service.to_owned()),
                        config: None,
                        host: None,
                        port: None,
                    },
                )?;
                let reporter = AutomaticReporter::start(self.ctx.clone(), &prepared);
                let server = simile_server::Server::bind_durable_with_options(
                    prepared.report.listen,
                    prepared.artifact,
                    ops::service_state_path(&self.ctx, service)?,
                    simile_server::ServerOptions {
                        reporting_sender: reporter.sender(),
                        ..simile_server::ServerOptions::default()
                    },
                )
                .await?;
                let address = server.local_addr();
                let (shutdown, signal) = oneshot::channel::<()>();
                let task = tokio::spawn(async move {
                    let result = server
                        .run_until(async {
                            drop(signal.await);
                        })
                        .await;
                    reporter.finish().await;
                    result
                });
                self.runtime = Some(SelectedRuntime {
                    service: service.to_owned(),
                    address,
                    ownership: RuntimeOwnership::Owned { shutdown, task },
                });
                return Ok(());
            }
        };
        self.runtime = Some(SelectedRuntime {
            service: service.to_owned(),
            address,
            ownership,
        });
        Ok(())
    }

    async fn stop_owned_runtime(&mut self) {
        let Some(runtime) = self.runtime.take() else {
            return;
        };
        debug_assert!(!runtime.service.is_empty());
        if let RuntimeOwnership::Owned { shutdown, task } = runtime.ownership {
            let _ = shutdown.send(());
            match task.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    tracing::warn!(%error, "console-owned service stopped with an error")
                }
                Err(error) => tracing::warn!(%error, "console-owned service task failed"),
            }
        }
    }
}

fn inspection_matches(
    identity: &InspectionIdentity,
    registered_name: &str,
    selected: &ops::ConsoleService,
) -> bool {
    identity.artifact_digest.as_ref().map_or_else(
        || {
            identity.service == registered_name
                || (identity.service_version == selected.service.version
                    && identity.operations == selected.metrics.operations)
        },
        |digest| digest == &selected.artifact.sha256,
    )
}

struct RawResponse {
    status: u16,
    body: Vec<u8>,
}

async fn probe(address: SocketAddr) -> Probe {
    let response = tokio::time::timeout(
        Duration::from_millis(350),
        raw_request(address, "GET", "/.simile/inspection", None),
    )
    .await;
    match response {
        Err(_) | Ok(Err(_)) => Probe::Unreachable,
        Ok(Ok(response)) if response.status == 200 => {
            serde_json::from_slice(&response.body).map_or(Probe::Other, Probe::Service)
        }
        Ok(Ok(_)) => Probe::Other,
    }
}

async fn raw_request(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> std::io::Result<RawResponse> {
    let mut stream = tokio::net::TcpStream::connect(address).await?;
    let body = body.unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(body).await?;
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await?;
    let header_end = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map_or(bytes.len(), |index| index + 4);
    let status = String::from_utf8_lossy(&bytes[..header_end])
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or_default();
    Ok(RawResponse {
        status,
        body: bytes[header_end..].to_vec(),
    })
}
