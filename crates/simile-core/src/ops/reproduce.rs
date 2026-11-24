use std::collections::BTreeMap;
#[cfg(feature = "native")]
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use simile_runtime::{
    ExecutionOptions, IncomingParameters, OperationCall, OperationValueCall, Runtime,
    validate_parameters,
};
use simile_service::Service;

use super::run::{ExecutedRequest, ExecutedResponse, RunReport};
use crate::{Error, Result};

const RESPONSE_FORMAT_VERSION: u16 = 1;
const INCIDENT_FORMAT_VERSION: u16 = 2;
const EXCHANGE_FORMAT_VERSION: u16 = 3;
const MANIFEST: &str = "bundle.json";
const ARTIFACT: &str = "service.similec";
const REQUEST: &str = "request.json";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayRequest {
    pub bundle: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayReport {
    pub service: String,
    pub service_version: String,
    pub operation: String,
    pub seed: u64,
    pub artifact_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_simile_version: Option<String>,
    pub matched: bool,
    pub expected: ExecutedResponse,
    pub actual: ExecutedResponse,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    format_version: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    simile_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    artifact_format: Option<u16>,
    artifact_sha256: String,
    service: String,
    service_version: String,
    operation: String,
    seed: u64,
    outcome: Option<String>,
    scenario: Option<String>,
    expected: ExecutedResponse,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReproductionDiagnostic {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<serde_json::Value>,
}

#[derive(Clone, Debug)]
#[cfg(feature = "native")]
pub struct IncidentBundleRequest<'a> {
    pub path: &'a Path,
    pub artifact_bytes: &'a [u8],
    pub service: &'a str,
    pub service_version: &'a str,
    pub operation: &'a str,
    pub seed: u64,
    pub content_type: Option<&'a str>,
    pub body: &'a [u8],
    pub parameters: &'a IncomingParameters,
    pub scenario: Option<&'a str>,
    pub fault_profile: Option<&'a str>,
    pub expected: ReproductionDiagnostic,
}

#[derive(Clone, Debug)]
#[cfg(feature = "native")]
pub struct ExchangeBundleRequest<'a> {
    pub path: &'a Path,
    pub artifact_bytes: &'a [u8],
    pub service: &'a str,
    pub service_version: &'a str,
    pub operation: &'a str,
    pub response_seed: u64,
    pub logical_time_milliseconds: Option<i64>,
    pub content_type: Option<&'a str>,
    pub body: &'a [u8],
    pub parameters: &'a IncomingParameters,
    pub outcome: Option<&'a str>,
    pub scenario: Option<&'a str>,
    pub fault_profile: Option<&'a str>,
    pub expected: &'a ExecutedResponse,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExchangeManifest {
    format_version: u16,
    simile_version: String,
    artifact_format: u16,
    artifact_sha256: String,
    service: String,
    service_version: String,
    operation: String,
    response_seed: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    logical_time_milliseconds: Option<i64>,
    outcome: Option<String>,
    scenario: Option<String>,
    fault_profile: Option<String>,
    expected: ExecutedResponse,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct IncidentManifest {
    format_version: u16,
    simile_version: String,
    artifact_format: u16,
    artifact_sha256: String,
    service: String,
    service_version: String,
    operation: String,
    seed: u64,
    scenario: Option<String>,
    fault_profile: Option<String>,
    expected_error: ReproductionDiagnostic,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct IncidentRequestDocument {
    content_type: Option<String>,
    parameters: IncomingParameters,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExchangeRequestDocument {
    content_type: Option<String>,
    parameters: IncomingParameters,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaseVerification {
    pub service: String,
    pub service_version: String,
    pub operation: String,
    pub artifact_digest: String,
    pub bundle_simile_version: Option<String>,
    pub reproduced: bool,
    pub fingerprint: String,
    pub diagnostic: ReproductionDiagnostic,
}

#[cfg(feature = "native")]
pub(crate) fn write(
    path: &Path,
    artifact_bytes: &[u8],
    report: &RunReport,
    outcome: Option<&str>,
    scenario: Option<&str>,
) -> Result<()> {
    if path.exists() {
        return Err(Error::ReproductionExists {
            path: path.to_path_buf(),
        });
    }
    let bundle = response_bundle(
        path.to_path_buf(),
        artifact_bytes,
        report,
        outcome,
        scenario,
    )?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent).map_err(|source| io("create", parent, source))?;
    }
    let staging = staging_path(path);
    fs::create_dir(&staging).map_err(|source| io("create", &staging, source))?;
    let result = (|| {
        for (name, bytes) in &bundle.files {
            fs::write(staging.join(name), bytes)
                .map_err(|source| io("write", &staging.join(name), source))?;
        }
        fs::rename(&staging, path).map_err(|source| io("install", path, source))?;
        Ok(())
    })();
    if result.is_err() && staging.exists() {
        drop(fs::remove_dir_all(&staging));
    }
    result
}

#[cfg(feature = "native")]
pub fn replay(request: &ReplayRequest) -> Result<ReplayReport> {
    replay_bundle(&MemoryBundle::load(&request.bundle)?)
}

pub fn replay_bundle(bundle: &MemoryBundle) -> Result<ReplayReport> {
    let manifest_path = bundle.name.join(MANIFEST);
    let manifest: Manifest = serde_json::from_slice(bundle.file(MANIFEST)?).map_err(|source| {
        Error::ReproductionManifest {
            path: manifest_path,
            source,
        }
    })?;
    if manifest.format_version != RESPONSE_FORMAT_VERSION {
        return Err(Error::ReproductionFormat {
            found: manifest.format_version,
            supported: RESPONSE_FORMAT_VERSION,
        });
    }
    if manifest
        .artifact_format
        .is_some_and(|format| format != simile_artifact::FORMAT_VERSION)
    {
        return Err(Error::ReproductionArtifactFormat {
            found: manifest.artifact_format.unwrap_or_default(),
            supported: simile_artifact::FORMAT_VERSION,
        });
    }
    let artifact = simile_artifact::decode(bundle.file(ARTIFACT)?)?;
    if artifact.summary().payload_sha256 != manifest.artifact_sha256 {
        return Err(Error::ReproductionDigest);
    }
    let request_path = bundle.name.join(REQUEST);
    let executed_request: ExecutedRequest =
        serde_json::from_slice(bundle.file(REQUEST)?).map_err(|source| {
            Error::ReproductionManifest {
                path: request_path,
                source,
            }
        })?;
    let parameters =
        validate_parameters(&artifact, &manifest.operation, &executed_request.parameters)?;
    let execution = Runtime::from_artifact(artifact).execute_value_with_options(
        OperationValueCall {
            operation: &manifest.operation,
            content_type: executed_request.content_type.as_deref(),
            body: executed_request.body.as_ref(),
        },
        ExecutionOptions {
            parameters: Some(&parameters),
            outcome: manifest.outcome.as_deref(),
            scenario: manifest.scenario.as_deref(),
            response_seed: Some(super::run::response_seed(manifest.seed)),
            ..ExecutionOptions::default()
        },
    )?;
    let actual = ExecutedResponse {
        status: execution.response().status(),
        content_type: execution.response().media_type().map(str::to_owned),
        body: execution.response().value().cloned(),
    };
    Ok(ReplayReport {
        service: manifest.service,
        service_version: manifest.service_version,
        operation: manifest.operation,
        seed: manifest.seed,
        artifact_digest: manifest.artifact_sha256,
        bundle_simile_version: manifest.simile_version,
        matched: actual == manifest.expected,
        expected: manifest.expected,
        actual,
    })
}

/// Capture one sanitized runtime failure as a replayable case.
#[cfg(feature = "native")]
pub fn write_incident(request: &IncidentBundleRequest<'_>) -> Result<()> {
    if request.path.exists() {
        return Err(Error::ReproductionExists {
            path: request.path.to_path_buf(),
        });
    }
    let artifact = simile_artifact::decode(request.artifact_bytes)?;
    let manifest = IncidentManifest {
        format_version: INCIDENT_FORMAT_VERSION,
        simile_version: env!("CARGO_PKG_VERSION").to_owned(),
        artifact_format: simile_artifact::FORMAT_VERSION,
        artifact_sha256: artifact.summary().payload_sha256,
        service: request.service.to_owned(),
        service_version: request.service_version.to_owned(),
        operation: request.operation.to_owned(),
        seed: request.seed,
        scenario: request.scenario.map(str::to_owned),
        fault_profile: request.fault_profile.map(str::to_owned),
        expected_error: request.expected.clone(),
    };
    let document = IncidentRequestDocument {
        content_type: request.content_type.map(str::to_owned),
        parameters: request.parameters.clone(),
    };
    let parent = request
        .path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent).map_err(|source| io("create", parent, source))?;
    }
    let staging = staging_path(request.path);
    fs::create_dir(&staging).map_err(|source| io("create", &staging, source))?;
    let result = (|| {
        write_json(&staging.join(MANIFEST), &manifest)?;
        write_json(&staging.join(REQUEST), &document)?;
        fs::write(staging.join("request.body"), request.body)
            .map_err(|source| io("write", &staging.join("request.body"), source))?;
        fs::write(staging.join(ARTIFACT), request.artifact_bytes)
            .map_err(|source| io("write", &staging.join(ARTIFACT), source))?;
        fs::rename(&staging, request.path).map_err(|source| io("install", request.path, source))?;
        Ok(())
    })();
    if result.is_err() && staging.exists() {
        drop(fs::remove_dir_all(&staging));
    }
    result
}

/// Capture one sanitized HTTP exchange as a deterministic response reproduction.
#[cfg(feature = "native")]
pub fn write_exchange(request: &ExchangeBundleRequest<'_>) -> Result<()> {
    if request.path.exists() {
        return Err(Error::ReproductionExists {
            path: request.path.to_path_buf(),
        });
    }
    let artifact = simile_artifact::decode(request.artifact_bytes)?;
    let manifest = ExchangeManifest {
        format_version: EXCHANGE_FORMAT_VERSION,
        simile_version: env!("CARGO_PKG_VERSION").to_owned(),
        artifact_format: simile_artifact::FORMAT_VERSION,
        artifact_sha256: artifact.summary().payload_sha256,
        service: request.service.to_owned(),
        service_version: request.service_version.to_owned(),
        operation: request.operation.to_owned(),
        response_seed: request.response_seed,
        logical_time_milliseconds: request.logical_time_milliseconds,
        outcome: request.outcome.map(str::to_owned),
        scenario: request.scenario.map(str::to_owned),
        fault_profile: request.fault_profile.map(str::to_owned),
        expected: request.expected.clone(),
    };
    let document = ExchangeRequestDocument {
        content_type: request.content_type.map(str::to_owned),
        parameters: request.parameters.clone(),
    };
    let parent = request
        .path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent).map_err(|source| io("create", parent, source))?;
    }
    let staging = staging_path(request.path);
    fs::create_dir(&staging).map_err(|source| io("create", &staging, source))?;
    let result = (|| {
        write_json(&staging.join(MANIFEST), &manifest)?;
        write_json(&staging.join(REQUEST), &document)?;
        fs::write(staging.join("request.body"), request.body)
            .map_err(|source| io("write", &staging.join("request.body"), source))?;
        fs::write(staging.join(ARTIFACT), request.artifact_bytes)
            .map_err(|source| io("write", &staging.join(ARTIFACT), source))?;
        fs::rename(&staging, request.path).map_err(|source| io("install", request.path, source))?;
        Ok(())
    })();
    if result.is_err() && staging.exists() {
        drop(fs::remove_dir_all(&staging));
    }
    result
}

/// Verify either a response reproduction or an incident reproduction without trusting its claim.
#[cfg(feature = "native")]
pub fn verify_case(path: &Path) -> Result<CaseVerification> {
    verify_bundle(&MemoryBundle::load(path)?)
}

pub fn verify_bundle(bundle: &MemoryBundle) -> Result<CaseVerification> {
    let path = &bundle.name;
    let manifest_path = path.join(MANIFEST);
    let manifest_value: serde_json::Value = serde_json::from_slice(bundle.file(MANIFEST)?)
        .map_err(|source| Error::ReproductionManifest {
            path: manifest_path.clone(),
            source,
        })?;
    let format = manifest_value
        .get("formatVersion")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| Error::InvalidRegistration {
            service: "reproduction".to_owned(),
            reason: "bundle manifest has no valid formatVersion".to_owned(),
        })?;
    if format == RESPONSE_FORMAT_VERSION {
        let replayed = replay_bundle(bundle)?;
        let diagnostic = ReproductionDiagnostic {
            code: "emulation_inconsistency".to_owned(),
            message: "the recorded response is reproduced".to_owned(),
            issues: Vec::new(),
        };
        return Ok(CaseVerification {
            service: replayed.service,
            service_version: replayed.service_version,
            operation: replayed.operation.clone(),
            artifact_digest: replayed.artifact_digest.clone(),
            bundle_simile_version: replayed.bundle_simile_version,
            reproduced: replayed.matched,
            fingerprint: format!(
                "{}:{}:{}",
                replayed.artifact_digest, replayed.operation, replayed.seed
            ),
            diagnostic,
        });
    }
    if format == EXCHANGE_FORMAT_VERSION {
        let manifest: ExchangeManifest =
            serde_json::from_value(manifest_value).map_err(|source| {
                Error::ReproductionManifest {
                    path: manifest_path,
                    source,
                }
            })?;
        if manifest.artifact_format != simile_artifact::FORMAT_VERSION {
            return Err(Error::ReproductionArtifactFormat {
                found: manifest.artifact_format,
                supported: simile_artifact::FORMAT_VERSION,
            });
        }
        let artifact = simile_artifact::decode(bundle.file(ARTIFACT)?)?;
        if artifact.summary().payload_sha256 != manifest.artifact_sha256 {
            return Err(Error::ReproductionDigest);
        }
        let request_path = path.join(REQUEST);
        let request: ExchangeRequestDocument = serde_json::from_slice(bundle.file(REQUEST)?)
            .map_err(|source| Error::ReproductionManifest {
                path: request_path,
                source,
            })?;
        let body = bundle.file("request.body")?;
        let parameters = validate_parameters(&artifact, &manifest.operation, &request.parameters)?;
        let service = Service::memory(artifact)?;
        let execution = service.execute_with_options(
            OperationCall {
                operation: &manifest.operation,
                content_type: request.content_type.as_deref(),
                body,
            },
            ExecutionOptions {
                parameters: Some(&parameters),
                outcome: manifest.outcome.as_deref(),
                scenario: manifest.scenario.as_deref(),
                fault_profile: manifest.fault_profile.as_deref(),
                response_seed: Some(manifest.response_seed),
                logical_time_milliseconds: manifest.logical_time_milliseconds,
                ..ExecutionOptions::default()
            },
        );
        let mut actual = match execution {
            Ok(execution) => ExecutedResponse {
                status: execution.response().status(),
                content_type: execution.response().media_type().map(str::to_owned),
                body: execution.response().value().cloned(),
            },
            Err(simile_service::Error::Runtime(error)) => return Err(error.into()),
            Err(_) => ExecutedResponse {
                status: 500,
                content_type: Some("application/json".to_owned()),
                body: Some(serde_json::json!({
                    "error": {
                        "code": "effect_failed",
                        "message": "the operation completed but its declared effects could not be committed"
                    }
                })),
            },
        };
        if let Some(body) = actual.body.as_mut() {
            redact_exchange_response(body);
        }
        let reproduced = actual == manifest.expected;
        return Ok(CaseVerification {
            service: manifest.service,
            service_version: manifest.service_version,
            operation: manifest.operation.clone(),
            artifact_digest: manifest.artifact_sha256.clone(),
            bundle_simile_version: Some(manifest.simile_version),
            reproduced,
            fingerprint: format!(
                "{}:{}:{}",
                manifest.artifact_sha256, manifest.operation, manifest.response_seed
            ),
            diagnostic: ReproductionDiagnostic {
                code: "emulation_inconsistency".to_owned(),
                message: "the captured HTTP exchange is reproduced".to_owned(),
                issues: Vec::new(),
            },
        });
    }
    if format != INCIDENT_FORMAT_VERSION {
        return Err(Error::ReproductionFormat {
            found: format,
            supported: EXCHANGE_FORMAT_VERSION,
        });
    }
    let manifest: IncidentManifest =
        serde_json::from_value(manifest_value).map_err(|source| Error::ReproductionManifest {
            path: manifest_path,
            source,
        })?;
    if manifest.artifact_format != simile_artifact::FORMAT_VERSION {
        return Err(Error::ReproductionArtifactFormat {
            found: manifest.artifact_format,
            supported: simile_artifact::FORMAT_VERSION,
        });
    }
    let artifact = simile_artifact::decode(bundle.file(ARTIFACT)?)?;
    if artifact.summary().payload_sha256 != manifest.artifact_sha256 {
        return Err(Error::ReproductionDigest);
    }
    let request_path = path.join(REQUEST);
    let request: IncidentRequestDocument =
        serde_json::from_slice(bundle.file(REQUEST)?).map_err(|source| {
            Error::ReproductionManifest {
                path: request_path,
                source,
            }
        })?;
    let body = bundle.file("request.body")?;
    let parameters = validate_parameters(&artifact, &manifest.operation, &request.parameters)?;
    let actual = Runtime::from_artifact(artifact).execute_with_options(
        OperationCall {
            operation: &manifest.operation,
            content_type: request.content_type.as_deref(),
            body,
        },
        ExecutionOptions {
            parameters: Some(&parameters),
            scenario: manifest.scenario.as_deref(),
            fault_profile: manifest.fault_profile.as_deref(),
            response_seed: Some(manifest.seed),
            ..ExecutionOptions::default()
        },
    );
    let actual_diagnostic = actual.err().map(|error| ReproductionDiagnostic {
        code: error.code().to_owned(),
        message: error.to_string(),
        issues: error
            .issues()
            .iter()
            .filter_map(|issue| serde_json::to_value(issue).ok())
            .collect(),
    });
    let reproduced = actual_diagnostic.as_ref() == Some(&manifest.expected_error);
    let fingerprint = diagnostic_fingerprint(
        &manifest.artifact_sha256,
        &manifest.operation,
        &manifest.expected_error,
    );
    Ok(CaseVerification {
        service: manifest.service,
        service_version: manifest.service_version,
        operation: manifest.operation,
        artifact_digest: manifest.artifact_sha256,
        bundle_simile_version: Some(manifest.simile_version),
        reproduced,
        fingerprint,
        diagnostic: manifest.expected_error,
    })
}

fn diagnostic_fingerprint(
    artifact_digest: &str,
    operation: &str,
    diagnostic: &ReproductionDiagnostic,
) -> String {
    format!(
        "{artifact_digest}:{operation}:{}:{}",
        diagnostic.code, diagnostic.message
    )
}

fn redact_exchange_response(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            for (name, value) in object {
                if sensitive_name(name) {
                    *value = serde_json::Value::String("<redacted>".to_owned());
                } else {
                    redact_exchange_response(value);
                }
            }
        }
        serde_json::Value::Array(values) => {
            values.iter_mut().for_each(redact_exchange_response);
        }
        _ => {}
    }
}

fn sensitive_name(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase().replace(['-', '.'], "_");
    [
        "authorization",
        "cookie",
        "password",
        "secret",
        "token",
        "api_key",
        "apikey",
        "access_key",
        "private_key",
        "card_number",
        "account_number",
        "cvc",
        "cvv",
    ]
    .iter()
    .any(|sensitive| normalized == *sensitive || normalized.ends_with(&format!("_{sensitive}")))
}

#[cfg(feature = "native")]
fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|source| Error::ReproductionManifest {
        path: path.to_path_buf(),
        source,
    })?;
    fs::write(path, bytes).map_err(|source| io("write", path, source))
}

#[cfg(feature = "native")]
fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|source| io("read", path, source))
}

fn io(action: &'static str, path: &Path, source: std::io::Error) -> Error {
    Error::ReproductionIo {
        action,
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(feature = "native")]
fn staging_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("bundle");
    path.with_file_name(format!(".{name}.tmp-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use simile_compiler::{CompileRequest, Compiler, Source};
    use simile_heuristics::Policy;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn incident_artifact() -> Vec<u8> {
        let source = r#"
openapi: 3.1.0
info: { title: Reports, version: 1.0.0 }
paths:
  /reports:
    post:
      operationId: createReport
      responses:
        '200':
          description: unsupported response media for the test
          content:
            application/x-unencodable:
              schema: { type: string }
"#;
        let compiled = Compiler::new()
            .compile(CompileRequest {
                openapi: Source::new("reports.yml", source),
                modules: Vec::new(),
                heuristic_policy: Policy::Standard,
            })
            .unwrap_or_else(|error| panic!("fixture must compile: {error}"));
        simile_artifact::encode(&compiled)
            .unwrap_or_else(|error| panic!("fixture must encode: {error}"))
    }

    fn exchange_artifact() -> Vec<u8> {
        let source = r#"
openapi: 3.1.0
info: { title: Exchange reports, version: 1.0.0 }
paths:
  /orders/{id}:
    get:
      operationId: lookupOrder
      parameters:
        - in: path
          name: id
          required: true
          schema: { type: string }
      responses:
        '200':
          description: order
          content:
            application/json:
              schema:
                type: object
                required: [id, status]
                properties:
                  id: { type: string }
                  status: { type: string, enum: [ready] }
"#;
        let compiled = Compiler::new()
            .compile(CompileRequest {
                openapi: Source::new("exchange-reports.yml", source),
                modules: Vec::new(),
                heuristic_policy: Policy::Standard,
            })
            .unwrap_or_else(|error| panic!("fixture must compile: {error}"));
        simile_artifact::encode(&compiled)
            .unwrap_or_else(|error| panic!("fixture must encode: {error}"))
    }

    fn failing_effect_artifact() -> Vec<u8> {
        let source = r#"
openapi: 3.1.0
info: { title: Effect reports, version: 1.0.0 }
paths:
  /schedule:
    post:
      operationId: scheduleWork
      responses:
        '200':
          description: scheduled
          content:
            application/json:
              schema:
                type: object
                required: [id]
                properties:
                  id: { type: string, const: not-a-time }
"#;
        let behavior = r#"
backend "scheduler" "work" {}

effect "schedule_work" {
  operations = ["scheduleWork"]

  action "scheduler.work" "schedule" {
    due_at  = response.id
    payload = response
  }
}
"#;
        let compiled = Compiler::new()
            .compile(CompileRequest {
                openapi: Source::new("effect-reports.yml", source),
                modules: vec![Source::new("effects.hcl", behavior)],
                heuristic_policy: Policy::Standard,
            })
            .unwrap_or_else(|error| panic!("fixture must compile: {error}"));
        simile_artifact::encode(&compiled)
            .unwrap_or_else(|error| panic!("fixture must encode: {error}"))
    }

    fn temporary_case() -> PathBuf {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "simile-incident-case-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn verifies_an_incident_against_its_exact_artifact_and_seed() {
        let path = temporary_case();
        drop(fs::remove_dir_all(&path));
        let artifact_bytes = incident_artifact();
        let artifact = simile_artifact::decode(&artifact_bytes)
            .unwrap_or_else(|error| panic!("fixture must decode: {error}"));
        // Why: the workspace denies `expect` even in tests; an explicit match keeps the failure
        // message while preserving the exact runtime error needed to build the incident fixture.
        let error = match Runtime::from_artifact(artifact).execute_with_options(
            OperationCall {
                operation: "POST /reports",
                content_type: None,
                body: b"",
            },
            ExecutionOptions {
                response_seed: Some(17),
                ..ExecutionOptions::default()
            },
        ) {
            Err(error) => error,
            Ok(_) => panic!("the fixture must produce an internal response error"),
        };
        let expected = ReproductionDiagnostic {
            code: error.code().to_owned(),
            message: error.to_string(),
            issues: Vec::new(),
        };

        write_incident(&IncidentBundleRequest {
            path: &path,
            artifact_bytes: &artifact_bytes,
            service: "Reports",
            service_version: "1.0.0",
            operation: "POST /reports",
            seed: 17,
            content_type: None,
            body: b"",
            parameters: &IncomingParameters::default(),
            scenario: None,
            fault_profile: None,
            expected: expected.clone(),
        })
        .unwrap_or_else(|error| panic!("incident must be written: {error}"));

        let verified =
            verify_case(&path).unwrap_or_else(|error| panic!("incident must verify: {error}"));
        assert!(verified.reproduced);
        assert_eq!(verified.diagnostic, expected);
        assert_eq!(verified.operation, "POST /reports");
        assert!(verified.fingerprint.contains(&verified.diagnostic.code));
        drop(fs::remove_dir_all(path));
    }

    #[test]
    fn verifies_a_captured_exchange_with_its_parameters_and_response_seed() {
        let path = temporary_case();
        drop(fs::remove_dir_all(&path));
        let artifact_bytes = exchange_artifact();
        let artifact = simile_artifact::decode(&artifact_bytes)
            .unwrap_or_else(|error| panic!("fixture must decode: {error}"));
        let mut parameters = IncomingParameters::default();
        parameters
            .path
            .insert("id".to_owned(), vec!["or_test".to_owned()]);
        let validated = validate_parameters(&artifact, "GET /orders/{id}", &parameters)
            .unwrap_or_else(|error| panic!("fixture parameters must validate: {error}"));
        let execution = Runtime::from_artifact(artifact)
            .execute_with_options(
                OperationCall {
                    operation: "GET /orders/{id}",
                    content_type: None,
                    body: b"",
                },
                ExecutionOptions {
                    parameters: Some(&validated),
                    response_seed: Some(41),
                    ..ExecutionOptions::default()
                },
            )
            .unwrap_or_else(|error| panic!("fixture exchange must execute: {error}"));
        let expected = ExecutedResponse {
            status: execution.response().status(),
            content_type: execution.response().media_type().map(str::to_owned),
            body: execution.response().value().cloned(),
        };

        write_exchange(&ExchangeBundleRequest {
            path: &path,
            artifact_bytes: &artifact_bytes,
            service: "Exchange reports",
            service_version: "1.0.0",
            operation: "GET /orders/{id}",
            response_seed: 41,
            logical_time_milliseconds: None,
            content_type: None,
            body: b"",
            parameters: &parameters,
            outcome: None,
            scenario: None,
            fault_profile: None,
            expected: &expected,
        })
        .unwrap_or_else(|error| panic!("exchange bundle must be written: {error}"));

        let verified =
            verify_case(&path).unwrap_or_else(|error| panic!("exchange must verify: {error}"));
        assert!(verified.reproduced);
        assert_eq!(verified.operation, "GET /orders/{id}");
        assert_eq!(
            verified.diagnostic.message,
            "the captured HTTP exchange is reproduced"
        );
        drop(fs::remove_dir_all(path));
    }

    #[test]
    fn verifies_a_captured_effect_commit_failure() {
        let path = temporary_case();
        drop(fs::remove_dir_all(&path));
        let artifact_bytes = failing_effect_artifact();
        let expected = ExecutedResponse {
            status: 500,
            content_type: Some("application/json".to_owned()),
            body: Some(serde_json::json!({
                "error": {
                    "code": "effect_failed",
                    "message": "the operation completed but its declared effects could not be committed"
                }
            })),
        };

        write_exchange(&ExchangeBundleRequest {
            path: &path,
            artifact_bytes: &artifact_bytes,
            service: "Effect reports",
            service_version: "1.0.0",
            operation: "POST /schedule",
            response_seed: 53,
            logical_time_milliseconds: None,
            content_type: None,
            body: b"",
            parameters: &IncomingParameters::default(),
            outcome: None,
            scenario: None,
            fault_profile: None,
            expected: &expected,
        })
        .unwrap_or_else(|error| panic!("exchange bundle must be written: {error}"));

        let verified = verify_case(&path)
            .unwrap_or_else(|error| panic!("effect failure must verify: {error}"));
        assert!(verified.reproduced);
        assert_eq!(verified.operation, "POST /schedule");
        drop(fs::remove_dir_all(path));
    }
}

/// Named bundle files. Payloads are canonical Rust JSON or raw artifact/body bytes.
#[derive(Clone, Debug)]
pub struct MemoryBundle {
    pub name: PathBuf,
    pub files: BTreeMap<String, Vec<u8>>,
}

impl MemoryBundle {
    fn file(&self, name: &str) -> Result<&[u8]> {
        self.files.get(name).map(Vec::as_slice).ok_or_else(|| {
            io(
                "read",
                &self.name.join(name),
                std::io::Error::from(std::io::ErrorKind::NotFound),
            )
        })
    }

    #[cfg(feature = "native")]
    pub fn load(path: &Path) -> Result<Self> {
        let mut files = BTreeMap::new();
        for name in [MANIFEST, ARTIFACT, REQUEST] {
            files.insert(name.to_owned(), read(&path.join(name))?);
        }
        if path.join("request.body").is_file() {
            files.insert("request.body".to_owned(), read(&path.join("request.body"))?);
        }
        Ok(Self {
            name: path.to_owned(),
            files,
        })
    }
}

pub fn response_bundle(
    name: PathBuf,
    artifact_bytes: &[u8],
    report: &RunReport,
    outcome: Option<&str>,
    scenario: Option<&str>,
) -> Result<MemoryBundle> {
    let artifact = simile_artifact::decode(artifact_bytes)?;
    let manifest = Manifest {
        format_version: RESPONSE_FORMAT_VERSION,
        simile_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        artifact_format: Some(simile_artifact::FORMAT_VERSION),
        artifact_sha256: artifact.summary().payload_sha256,
        service: report.service.clone(),
        service_version: report.service_version.clone(),
        operation: report.operation.clone(),
        seed: report.seed,
        outcome: outcome.map(str::to_owned),
        scenario: scenario.map(str::to_owned),
        expected: report.response.clone(),
    };

    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|source| Error::ReproductionManifest {
            path: name.join(MANIFEST),
            source,
        })?;
    let request_bytes = serde_json::to_vec_pretty(&report.request).map_err(|source| {
        Error::ReproductionManifest {
            path: name.join(REQUEST),
            source,
        }
    })?;
    Ok(MemoryBundle {
        name,
        files: BTreeMap::from([
            (MANIFEST.to_owned(), manifest_bytes),
            (REQUEST.to_owned(), request_bytes),
            (ARTIFACT.to_owned(), artifact_bytes.to_vec()),
        ]),
    })
}
