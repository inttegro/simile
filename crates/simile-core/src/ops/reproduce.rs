use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use simile_runtime::{ExecutionOptions, OperationValueCall, Runtime};

use super::run::{ExecutedRequest, ExecutedResponse, RunReport};
use crate::{Error, Result};

const FORMAT_VERSION: u16 = 1;
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
    let artifact = simile_artifact::decode(artifact_bytes)?;
    let manifest = Manifest {
        format_version: FORMAT_VERSION,
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
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent).map_err(|source| io("create", parent, source))?;
    }
    let staging = staging_path(path);
    fs::create_dir(&staging).map_err(|source| io("create", &staging, source))?;
    let result = (|| {
        write_json(&staging.join(MANIFEST), &manifest)?;
        write_json(&staging.join(REQUEST), &report.request)?;
        fs::write(staging.join(ARTIFACT), artifact_bytes)
            .map_err(|source| io("write", &staging.join(ARTIFACT), source))?;
        fs::rename(&staging, path).map_err(|source| io("install", path, source))?;
        Ok(())
    })();
    if result.is_err() && staging.exists() {
        drop(fs::remove_dir_all(&staging));
    }
    result
}

pub fn replay(request: &ReplayRequest) -> Result<ReplayReport> {
    let manifest_path = request.bundle.join(MANIFEST);
    let manifest: Manifest = serde_json::from_slice(&read(&manifest_path)?).map_err(|source| {
        Error::ReproductionManifest {
            path: manifest_path,
            source,
        }
    })?;
    if manifest.format_version != FORMAT_VERSION {
        return Err(Error::ReproductionFormat {
            found: manifest.format_version,
            supported: FORMAT_VERSION,
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
    let artifact = simile_artifact::decode(&read(&request.bundle.join(ARTIFACT))?)?;
    if artifact.summary().payload_sha256 != manifest.artifact_sha256 {
        return Err(Error::ReproductionDigest);
    }
    let request_path = request.bundle.join(REQUEST);
    let executed_request: ExecutedRequest =
        serde_json::from_slice(&read(&request_path)?).map_err(|source| {
            Error::ReproductionManifest {
                path: request_path,
                source,
            }
        })?;
    let execution = Runtime::from_artifact(artifact).execute_value_with_options(
        OperationValueCall {
            operation: &manifest.operation,
            content_type: executed_request.content_type.as_deref(),
            body: executed_request.body.as_ref(),
        },
        ExecutionOptions {
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
        matched: actual == manifest.expected,
        expected: manifest.expected,
        actual,
    })
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|source| Error::ReproductionManifest {
        path: path.to_path_buf(),
        source,
    })?;
    fs::write(path, bytes).map_err(|source| io("write", path, source))
}

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

fn staging_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("bundle");
    path.with_file_name(format!(".{name}.tmp-{}", std::process::id()))
}
