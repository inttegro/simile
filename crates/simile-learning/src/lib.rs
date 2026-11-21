#![forbid(unsafe_code)]

mod contract;
mod privacy;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("could not {action} learning workspace path `{path}`: {source}")]
    WorkspaceIo {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not encode learning evidence: {0}")]
    EvidenceEncode(#[source] serde_json::Error),

    #[error("could not encode learned OpenAPI contract: {0}")]
    OpenApiEncode(#[source] serde_norway::Error),

    #[error("learning session state is unavailable")]
    SessionState,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Api,
    Webhook,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedMessage {
    pub headers: BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    pub body: ObservedBody,
    pub bytes: usize,
}

impl ObservedMessage {
    #[must_use]
    pub fn capture(
        headers: impl IntoIterator<Item = (String, Vec<String>)>,
        content_type: Option<String>,
        bytes: &[u8],
    ) -> Self {
        let headers = privacy::sanitize_headers(headers);
        let body = privacy::sanitize_body(content_type.as_deref(), bytes);
        Self {
            headers,
            content_type,
            body,
            bytes: bytes.len(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObservedBody {
    Empty,
    Json { value: Value },
    Text,
    Opaque,
}

impl ObservedBody {
    #[must_use]
    pub const fn json(&self) -> Option<&Value> {
        match self {
            Self::Json { value } => Some(value),
            Self::Empty | Self::Text | Self::Opaque => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exchange {
    pub sequence: u64,
    pub observed_at_milliseconds: u128,
    pub direction: Direction,
    pub method: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub request: ObservedMessage,
    pub status: u16,
    pub response: ObservedMessage,
    pub elapsed_microseconds: u128,
}

#[derive(Clone, Debug)]
pub struct ExchangeInput {
    pub direction: Direction,
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub request: ObservedMessage,
    pub status: u16,
    pub response: ObservedMessage,
    pub elapsed: Duration,
}

#[derive(Clone)]
pub struct Session {
    inner: Arc<Mutex<SessionState>>,
}

struct SessionState {
    service: String,
    session: String,
    workspace: PathBuf,
    evidence: BufWriter<File>,
    exchanges: Vec<Exchange>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningReport {
    pub service: String,
    pub session: String,
    pub workspace: PathBuf,
    pub exchanges: usize,
    pub api_requests: usize,
    pub webhook_deliveries: usize,
    pub operations: usize,
    pub statuses: usize,
    pub bindings: usize,
    pub unknowns: usize,
    pub simfile: PathBuf,
    pub openapi: PathBuf,
    pub behavior: PathBuf,
    pub evidence: PathBuf,
    pub report: PathBuf,
}

impl Session {
    pub fn create(
        service: impl Into<String>,
        session: impl Into<String>,
        workspace: impl Into<PathBuf>,
    ) -> Result<Self> {
        let service = service.into();
        let session = session.into();
        let workspace = workspace.into();
        let evidence_directory = workspace.join("evidence");
        fs::create_dir_all(&evidence_directory).map_err(|source| Error::WorkspaceIo {
            action: "create",
            path: evidence_directory.clone(),
            source,
        })?;
        let evidence_path = evidence_directory.join("observations.jsonl");
        let evidence = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&evidence_path)
            .map_err(|source| Error::WorkspaceIo {
                action: "open",
                path: evidence_path,
                source,
            })?;
        Ok(Self {
            inner: Arc::new(Mutex::new(SessionState {
                service,
                session,
                workspace,
                evidence: BufWriter::new(evidence),
                exchanges: Vec::new(),
            })),
        })
    }

    pub fn record(&self, input: ExchangeInput) -> Result<()> {
        let mut state = self.inner.lock().map_err(|_| Error::SessionState)?;
        let sequence = u64::try_from(state.exchanges.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let observed_at_milliseconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let exchange = Exchange {
            sequence,
            observed_at_milliseconds,
            direction: input.direction,
            method: input.method,
            path: input.path,
            query: input.query.as_deref().map(privacy::sanitize_query),
            request: input.request,
            status: input.status,
            response: input.response,
            elapsed_microseconds: input.elapsed.as_micros(),
        };
        serde_json::to_writer(&mut state.evidence, &exchange).map_err(Error::EvidenceEncode)?;
        state
            .evidence
            .write_all(b"\n")
            .map_err(|source| Error::WorkspaceIo {
                action: "append to",
                path: state.workspace.join("evidence/observations.jsonl"),
                source,
            })?;
        state
            .evidence
            .flush()
            .map_err(|source| Error::WorkspaceIo {
                action: "flush",
                path: state.workspace.join("evidence/observations.jsonl"),
                source,
            })?;
        state.exchanges.push(exchange);
        Ok(())
    }

    pub fn finish(&self, upstream: &str, listen_port: u16) -> Result<LearningReport> {
        let state = self.inner.lock().map_err(|_| Error::SessionState)?;
        let inferred = contract::infer(&state.service, upstream, &state.session, &state.exchanges);
        let service_directory = state.workspace.join("service");
        fs::create_dir_all(&service_directory).map_err(|source| Error::WorkspaceIo {
            action: "create",
            path: service_directory.clone(),
            source,
        })?;
        let openapi = service_directory.join("openapi.yaml");
        let openapi_bytes = serde_norway::to_string(&inferred.document)
            .map_err(Error::OpenApiEncode)?
            .into_bytes();
        write(&openapi, &openapi_bytes)?;

        let behavior = service_directory.join("behavior.hcl");
        write(
            &behavior,
            b"# Add provider-reviewed behavior that cannot be proved from captured HTTP exchanges.\n",
        )?;

        let simfile = service_directory.join("Simfile");
        let simfile_source = format!(
            "service \"{}\" {{\n  description = \"Learned from observed public HTTP behavior.\"\n\n  listen {{\n    host = \"127.0.0.1\"\n    port = {listen_port}\n  }}\n\n  openapi {{\n    path = \"./openapi.yaml\"\n  }}\n\n  behaviors = [\"./behavior.hcl\"]\n}}\n",
            state.service
        );
        write(&simfile, simfile_source.as_bytes())?;

        let api_requests = state
            .exchanges
            .iter()
            .filter(|exchange| exchange.direction == Direction::Api)
            .count();
        let webhook_deliveries = state.exchanges.len().saturating_sub(api_requests);
        let operations = state
            .exchanges
            .iter()
            .filter(|exchange| exchange.direction == Direction::Api)
            .map(|exchange| (exchange.method.as_str(), exchange.path.as_str()))
            .collect::<BTreeSet<_>>()
            .len();
        let statuses = state
            .exchanges
            .iter()
            .map(|exchange| exchange.status)
            .collect::<BTreeSet<_>>()
            .len();
        let evidence = state.workspace.join("evidence/observations.jsonl");
        let report_path = state.workspace.join("learning-report.json");
        let report = LearningReport {
            service: state.service.clone(),
            session: state.session.clone(),
            workspace: state.workspace.clone(),
            exchanges: state.exchanges.len(),
            api_requests,
            webhook_deliveries,
            operations,
            statuses,
            bindings: inferred.bindings,
            unknowns: inferred.unknowns,
            simfile,
            openapi,
            behavior,
            evidence,
            report: report_path.clone(),
        };
        let report_bytes = serde_json::to_vec_pretty(&report).map_err(Error::EvidenceEncode)?;
        write(&report_path, &report_bytes)?;
        Ok(report)
    }

    #[must_use]
    pub fn exchange_count(&self) -> usize {
        self.inner
            .lock()
            .map(|state| state.exchanges.len())
            .unwrap_or_default()
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes).map_err(|source| Error::WorkspaceIo {
        action: "write",
        path: path.to_owned(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    fn message(value: Value) -> ObservedMessage {
        ObservedMessage {
            headers: BTreeMap::new(),
            content_type: Some("application/json".to_owned()),
            bytes: value.to_string().len(),
            body: ObservedBody::Json { value },
        }
    }

    #[test]
    fn finishes_with_a_reviewable_service_workspace() {
        let root = std::env::temp_dir().join(format!(
            "simile-learning-finish-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let session =
            Session::create("acme", "learn_test", &root).unwrap_or_else(|error| panic!("{error}"));
        session
            .record(ExchangeInput {
                direction: Direction::Api,
                method: "POST".to_owned(),
                path: "/customers".to_owned(),
                query: Some("expand=name&api_key=secret".to_owned()),
                request: message(json!({ "name": "Ada" })),
                status: 201,
                response: message(json!({ "id": "cu_123", "name": "Ada" })),
                elapsed: Duration::from_millis(4),
            })
            .unwrap_or_else(|error| panic!("{error}"));

        let report = session
            .finish("https://api.acme.test", 8484)
            .unwrap_or_else(|error| panic!("{error}"));
        let openapi = fs::read_to_string(&report.openapi).unwrap_or_else(|error| panic!("{error}"));
        simile_openapi::parse(&openapi).unwrap_or_else(|error| panic!("{error}"));
        assert!(report.simfile.is_file());
        assert!(report.behavior.is_file());
        assert!(report.evidence.is_file());
        assert!(report.report.is_file());
        assert!(
            fs::read_to_string(&report.evidence)
                .unwrap_or_else(|error| panic!("{error}"))
                .contains("api_key=%3Credacted%3E")
        );
        drop(fs::remove_dir_all(root));
    }
}
