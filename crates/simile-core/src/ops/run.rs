use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use simile_heuristics::Policy;
use simile_runtime::{OperationValueCall, Runtime, SynthesisRequest, synthesize_request_value};

use super::compile::compile_service;
use crate::{Error, Result};

const MAX_REMOTE_SPEC_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpecSource {
    Path(PathBuf),
    Url(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationSelector {
    Key(String),
    Id(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequestInput {
    Inline(String),
    Path(PathBuf),
    Stdin,
}

impl RequestInput {
    #[must_use]
    pub fn from_argument(value: String) -> Self {
        if value == "-" {
            return Self::Stdin;
        }
        if let Some(path) = value.strip_prefix('@') {
            return Self::Path(PathBuf::from(path));
        }
        if value.trim_start().starts_with('{') {
            return Self::Inline(value);
        }
        Self::Path(PathBuf::from(value))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunRequest {
    pub spec: SpecSource,
    pub behaviors: Vec<PathBuf>,
    pub operation: OperationSelector,
    pub request: Option<RequestInput>,
    pub content_type: Option<String>,
    pub heuristic_policy: Policy,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestDocument {
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "deserialize_request_body")]
    body: RequestBody,
}

#[derive(Debug, Default)]
enum RequestBody {
    #[default]
    Absent,
    Present(Value),
}

#[derive(Debug)]
struct PreparedRequest {
    source: RequestSource,
    headers: BTreeMap<String, String>,
    content_type: Option<String>,
    body: Option<Value>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestSource {
    None,
    Synthesized,
    Provided,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutedRequest {
    pub source: RequestSource,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutedResponse {
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedAsyncOperation {
    pub name: String,
    pub resources: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub service: String,
    pub service_version: String,
    pub operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    pub request: ExecutedRequest,
    pub response: ExecutedResponse,
    pub async_operations: Vec<PlannedAsyncOperation>,
}

/// Compile a service source in memory and execute one selected operation.
pub fn run(request: &RunRequest) -> Result<RunReport> {
    let (source_name, source) = read_spec(&request.spec)?;
    let compiled = compile_service(
        &source_name,
        &source,
        &request.behaviors,
        request.heuristic_policy,
    )?;
    let service = compiled.contract.info.title.clone();
    let service_version = compiled.contract.info.version.clone();
    let encoded = simile_artifact::encode(&compiled)?;
    let artifact = simile_artifact::decode(&encoded)?;
    let selected =
        match &request.operation {
            OperationSelector::Key(key) => {
                artifact
                    .operation(key)
                    .ok_or_else(|| Error::UnknownOperation {
                        operation: key.clone(),
                    })?
            }
            OperationSelector::Id(operation_id) => artifact
                .operation_by_id(operation_id)
                .ok_or_else(|| Error::UnknownOperationId {
                    operation_id: operation_id.clone(),
                })?,
        };
    let operation = selected.key().to_owned();
    let operation_id = selected.operation_id().map(str::to_owned);
    let prepared = prepare_request(&artifact, &operation, request)?;
    let outcome = Runtime::from_artifact(artifact).execute_value(OperationValueCall {
        operation: &operation,
        content_type: prepared.content_type.as_deref(),
        body: prepared.body.as_ref(),
    })?;
    let executed_request = ExecutedRequest {
        source: prepared.source,
        headers: prepared.headers,
        content_type: outcome
            .request_body()
            .map(|request| request.media_type().to_owned()),
        body: outcome
            .request_body()
            .map(|request| request.value().clone()),
    };
    let response = outcome.response();
    let executed_response = ExecutedResponse {
        status: response.status(),
        content_type: response.media_type().map(str::to_owned),
        body: response.value().cloned(),
    };
    let async_operations = outcome
        .async_operations()
        .iter()
        .map(|operation| PlannedAsyncOperation {
            name: operation.name().to_owned(),
            resources: operation
                .resources()
                .iter()
                .map(ToString::to_string)
                .collect(),
        })
        .collect();

    Ok(RunReport {
        service,
        service_version,
        operation,
        operation_id,
        request: executed_request,
        response: executed_response,
        async_operations,
    })
}

fn prepare_request(
    artifact: &simile_artifact::Artifact,
    operation: &str,
    request: &RunRequest,
) -> Result<PreparedRequest> {
    if let Some(input) = &request.request {
        let (source_name, bytes) = read_request(input)?;
        let document: RequestDocument =
            serde_json::from_slice(&bytes).map_err(|source| Error::InvalidRequest {
                source_name,
                source,
            })?;
        let content_type = header(&document.headers, "content-type")?.map(str::to_owned);
        let body = match document.body {
            RequestBody::Absent => None,
            RequestBody::Present(value) => Some(value),
        };
        return Ok(PreparedRequest {
            source: RequestSource::Provided,
            headers: document.headers,
            content_type,
            body,
        });
    }

    let selected = artifact
        .operation(operation)
        .ok_or_else(|| Error::UnknownOperation {
            operation: operation.to_owned(),
        })?;
    let Some(_) = selected.request_body() else {
        return Ok(PreparedRequest {
            source: RequestSource::None,
            headers: BTreeMap::new(),
            content_type: None,
            body: None,
        });
    };
    let synthesized = synthesize_request_value(
        artifact,
        SynthesisRequest {
            operation,
            content_type: request.content_type.as_deref(),
        },
    )?;
    Ok(PreparedRequest {
        source: RequestSource::Synthesized,
        headers: BTreeMap::new(),
        content_type: Some(synthesized.media_type().to_owned()),
        body: Some(synthesized.value().clone()),
    })
}

fn read_request(input: &RequestInput) -> Result<(String, Vec<u8>)> {
    match input {
        RequestInput::Inline(value) => Ok(("inline JSON".to_owned(), value.as_bytes().to_vec())),
        RequestInput::Path(path) => fs::read(path)
            .map(|bytes| (path.to_string_lossy().into_owned(), bytes))
            .map_err(|source| Error::ReadRequest {
                path: path.clone(),
                source,
            }),
        RequestInput::Stdin => {
            let mut bytes = Vec::new();
            std::io::stdin()
                .read_to_end(&mut bytes)
                .map_err(|source| Error::ReadRequestStdin { source })?;
            Ok(("stdin".to_owned(), bytes))
        }
    }
}

fn deserialize_request_body<'de, D>(deserializer: D) -> std::result::Result<RequestBody, D::Error>
where
    D: Deserializer<'de>,
{
    Value::deserialize(deserializer).map(RequestBody::Present)
}

fn header<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Result<Option<&'a str>> {
    let mut values = headers
        .iter()
        .filter(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str());
    let value = values.next();
    if values.next().is_some() {
        return Err(Error::DuplicateRequestHeader {
            name: name.to_owned(),
        });
    }
    Ok(value)
}

pub(crate) fn read_spec(source: &SpecSource) -> Result<(String, String)> {
    match source {
        SpecSource::Path(path) => {
            let contents = fs::read_to_string(path).map_err(|source| Error::ReadSource {
                path: path.clone(),
                source,
            })?;
            Ok((path.to_string_lossy().into_owned(), contents))
        }
        SpecSource::Url(url) => {
            let config = ureq::Agent::config_builder()
                .user_agent(concat!("simile/", env!("CARGO_PKG_VERSION")))
                .timeout_global(Some(Duration::from_secs(30)))
                .build();
            let agent: ureq::Agent = config.into();
            let mut response = agent
                .get(url)
                .call()
                .map_err(|source| Error::FetchSource { source })?;
            let contents = response
                .body_mut()
                .with_config()
                .limit(MAX_REMOTE_SPEC_BYTES)
                .read_to_string()
                .map_err(|source| Error::FetchSource { source })?;
            Ok(("remote OpenAPI source".to_owned(), contents))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::Path;
    use std::thread;

    use serde_json::json;

    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn request_documents_distinguish_an_omitted_body_from_json_null() {
        let omitted: RequestDocument =
            serde_json::from_str(r#"{"headers":{}}"#).unwrap_or_else(|error| panic!("{error}"));
        let null: RequestDocument = serde_json::from_str(r#"{"headers":{},"body":null}"#)
            .unwrap_or_else(|error| panic!("{error}"));

        assert!(matches!(omitted.body, RequestBody::Absent));
        assert!(matches!(null.body, RequestBody::Present(Value::Null)));
    }

    #[test]
    fn request_arguments_select_inline_path_and_standard_input_sources() {
        assert_eq!(
            RequestInput::from_argument(r#" {"headers":{}}"#.to_owned()),
            RequestInput::Inline(r#" {"headers":{}}"#.to_owned())
        );
        assert_eq!(
            RequestInput::from_argument("request.json".to_owned()),
            RequestInput::Path(PathBuf::from("request.json"))
        );
        assert_eq!(
            RequestInput::from_argument("@request.json".to_owned()),
            RequestInput::Path(PathBuf::from("request.json"))
        );
        assert_eq!(
            RequestInput::from_argument("-".to_owned()),
            RequestInput::Stdin
        );
    }

    #[test]
    fn synthesizes_a_request_when_no_request_is_provided() {
        let report = run(&RunRequest {
            spec: SpecSource::Path(fixture("run-orders.yaml")),
            behaviors: Vec::new(),
            operation: OperationSelector::Id("createOrder".to_owned()),
            request: None,
            content_type: None,
            heuristic_policy: Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(report.operation, "POST /orders");
        assert_eq!(report.operation_id.as_deref(), Some("createOrder"));
        assert_eq!(report.request.source, RequestSource::Synthesized);
        let request_body = report
            .request
            .body
            .as_ref()
            .unwrap_or_else(|| panic!("request body should be synthesized"));
        assert!(
            request_body["product"]
                .as_str()
                .is_some_and(|product| !product.is_empty())
        );
        assert!(
            request_body["quantity"]
                .as_i64()
                .is_some_and(|quantity| quantity >= 1)
        );
        assert_eq!(report.response.status, 201);
        let response_body = report
            .response
            .body
            .as_ref()
            .unwrap_or_else(|| panic!("response body should be synthesized"));
        assert_eq!(response_body["id"], "ord_simile");
        assert_eq!(response_body["product"], request_body["product"]);
        assert_eq!(response_body["quantity"], request_body["quantity"]);
    }

    #[test]
    fn validates_and_uses_a_provided_request_object() {
        let request =
            std::env::temp_dir().join(format!("simile-run-request-{}.json", std::process::id()));
        fs::write(
            &request,
            br#"{"headers":{"CoNtEnT-TyPe":"application/json","X-Test":"yes"},"body":{"product":"custom_jingle","quantity":4}}"#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let result = run(&RunRequest {
            spec: SpecSource::Path(fixture("run-orders.yaml")),
            behaviors: Vec::new(),
            operation: OperationSelector::Id("createOrder".to_owned()),
            request: Some(RequestInput::Path(request.clone())),
            content_type: None,
            heuristic_policy: Policy::Standard,
        });
        fs::remove_file(request).unwrap_or_else(|error| panic!("{error}"));
        let report = result.unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(report.request.source, RequestSource::Provided);
        assert_eq!(report.request.headers["X-Test"], "yes");
        assert_eq!(
            report.request.content_type.as_deref(),
            Some("application/json")
        );
        assert_eq!(
            report.request.body,
            Some(json!({"product": "custom_jingle", "quantity": 4}))
        );
        assert_eq!(
            report.response.body,
            Some(json!({
                "id": "ord_simile",
                "product": "custom_jingle",
                "quantity": 4
            }))
        );
    }

    #[test]
    fn a_provided_request_can_explicitly_omit_an_optional_body() {
        let request = std::env::temp_dir().join(format!(
            "simile-run-bodyless-request-{}.json",
            std::process::id()
        ));
        fs::write(&request, br#"{"headers":{"X-Test":"yes"}}"#)
            .unwrap_or_else(|error| panic!("{error}"));
        let result = run(&RunRequest {
            spec: SpecSource::Path(fixture("run-orders.yaml")),
            behaviors: Vec::new(),
            operation: OperationSelector::Id("optionalForm".to_owned()),
            request: Some(RequestInput::Path(request.clone())),
            content_type: None,
            heuristic_policy: Policy::Standard,
        });
        fs::remove_file(request).unwrap_or_else(|error| panic!("{error}"));
        let report = result.unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(report.request.source, RequestSource::Provided);
        assert_eq!(report.request.headers["X-Test"], "yes");
        assert_eq!(report.request.content_type, None);
        assert_eq!(report.request.body, None);
        assert_eq!(report.response.status, 204);
    }

    #[test]
    fn rejects_duplicate_case_insensitive_content_type_headers() {
        let request = std::env::temp_dir().join(format!(
            "simile-run-duplicate-header-request-{}.json",
            std::process::id()
        ));
        fs::write(
            &request,
            br#"{"headers":{"Content-Type":"application/json","content-type":"application/json"},"body":{"product":"custom_jingle","quantity":4}}"#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let result = run(&RunRequest {
            spec: SpecSource::Path(fixture("run-orders.yaml")),
            behaviors: Vec::new(),
            operation: OperationSelector::Id("createOrder".to_owned()),
            request: Some(RequestInput::Path(request.clone())),
            content_type: None,
            heuristic_policy: Policy::Standard,
        });
        fs::remove_file(request).unwrap_or_else(|error| panic!("{error}"));

        assert!(matches!(
            result,
            Err(Error::DuplicateRequestHeader { name }) if name == "content-type"
        ));
    }

    #[test]
    fn runs_a_bodyless_operation_by_structural_key() {
        let report = run(&RunRequest {
            spec: SpecSource::Path(fixture("run-orders.yaml")),
            behaviors: Vec::new(),
            operation: OperationSelector::Key("GET /health".to_owned()),
            request: None,
            content_type: None,
            heuristic_policy: Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(report.request.source, RequestSource::None);
        assert_eq!(report.request.body, None);
        assert_eq!(report.response.status, 204);
        assert_eq!(report.response.body, None);
    }

    #[test]
    fn synthesizes_an_optional_form_request_when_none_is_provided() {
        let report = run(&RunRequest {
            spec: SpecSource::Path(fixture("run-orders.yaml")),
            behaviors: Vec::new(),
            operation: OperationSelector::Id("optionalForm".to_owned()),
            request: None,
            content_type: None,
            heuristic_policy: Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(report.request.source, RequestSource::Synthesized);
        assert_eq!(
            report.request.content_type.as_deref(),
            Some("application/x-www-form-urlencoded")
        );
        assert!(
            report
                .request
                .body
                .as_ref()
                .and_then(|body| body["note"].as_str())
                .is_some_and(|note| !note.is_empty())
        );
        assert_eq!(report.response.status, 204);
    }

    #[test]
    fn reads_a_remote_spec_without_a_local_source_file() {
        let source = fs::read_to_string(fixture("run-orders.yaml"))
            .unwrap_or_else(|error| panic!("{error}"));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("{error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("{error}"));
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap_or_else(|error| panic!("{error}"));
            let mut request = [0_u8; 2048];
            let _ = stream
                .read(&mut request)
                .unwrap_or_else(|error| panic!("{error}"));
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/yaml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                source.len(),
                source
            )
            .unwrap_or_else(|error| panic!("{error}"));
        });

        let report = run(&RunRequest {
            spec: SpecSource::Url(format!("http://{address}/openapi.yaml")),
            behaviors: Vec::new(),
            operation: OperationSelector::Id("createOrder".to_owned()),
            request: None,
            content_type: None,
            heuristic_policy: Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));
        server
            .join()
            .unwrap_or_else(|error| std::panic::resume_unwind(error));

        assert_eq!(report.operation, "POST /orders");
        assert_eq!(report.request.source, RequestSource::Synthesized);
    }
}
