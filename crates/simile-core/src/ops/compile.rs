use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use simile_artifact::ArtifactSummary;
use simile_compiler::{Compiler, Source};
use simile_spec::{BehaviorOrigin, CompiledService};

use crate::{Error, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompileRequest {
    pub openapi: PathBuf,
    pub behaviors: Vec<PathBuf>,
    pub artifact: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WrittenArtifact {
    pub path: PathBuf,
    pub format_version: u16,
    pub bytes: usize,
    pub payload_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileReport {
    pub service: String,
    pub service_version: String,
    pub openapi_version: String,
    pub operations: usize,
    pub openapi_defaults: usize,
    pub hcl_behaviors: usize,
    pub async_operations: usize,
    pub resources: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact: Option<WrittenArtifact>,
}

/// Compile an OpenAPI contract and zero or more HCL behavior modules.
pub fn compile(request: &CompileRequest) -> Result<CompileReport> {
    let openapi = read(&request.openapi)?;
    let openapi_name = request.openapi.to_string_lossy().into_owned();
    let compiled = compile_service(&openapi_name, &openapi, &request.behaviors)?;
    let hcl_behaviors = compiled
        .operations
        .values()
        .filter(|operation| operation.origin == BehaviorOrigin::Hcl)
        .count();
    let artifact = request
        .artifact
        .as_ref()
        .map(|path| write_compiled_artifact(path, &compiled))
        .transpose()?;

    Ok(CompileReport {
        service: compiled.contract.info.title.clone(),
        service_version: compiled.contract.info.version.clone(),
        openapi_version: compiled.contract.openapi.exact.clone(),
        operations: compiled.operations.len(),
        openapi_defaults: compiled.operations.len() - hcl_behaviors,
        hcl_behaviors,
        async_operations: compiled.async_operations.len(),
        resources: compiled.resources.len(),
        artifact,
    })
}

pub(crate) fn compile_service(
    openapi_name: &str,
    openapi: &str,
    behavior_paths: &[PathBuf],
) -> Result<CompiledService> {
    let behavior_contents = behavior_paths
        .iter()
        .map(read)
        .collect::<Result<Vec<_>>>()?;
    let behavior_names = behavior_paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let behaviors = behavior_names
        .iter()
        .zip(&behavior_contents)
        .map(|(name, contents)| Source::new(name, contents))
        .collect();
    Compiler::new()
        .compile(simile_compiler::CompileRequest {
            openapi: Source::new(openapi_name, openapi),
            behaviors,
        })
        .map_err(Into::into)
}

fn write_compiled_artifact(
    path: &Path,
    compiled: &simile_spec::CompiledService,
) -> Result<WrittenArtifact> {
    let encoded = simile_artifact::encode(compiled)?;
    let summary = simile_artifact::decode(&encoded)?.summary();
    write_atomic(path, &encoded)?;
    Ok(written_artifact(path, &summary))
}

fn written_artifact(path: &Path, summary: &ArtifactSummary) -> WrittenArtifact {
    WrittenArtifact {
        path: path.to_owned(),
        format_version: summary.format_version,
        bytes: summary.encoded_bytes,
        payload_sha256: summary.payload_sha256.clone(),
    }
}

static WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artifact.similec");
    let sequence = WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        sequence
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        drop(fs::remove_file(&temporary));
    }
    result.map_err(|source| Error::WriteArtifact {
        path: path.to_owned(),
        source,
    })
}

fn read(path: &PathBuf) -> Result<String> {
    fs::read_to_string(path).map_err(|source| Error::ReadSource {
        path: path.clone(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn reports_compiled_units() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let report = compile(&CompileRequest {
            openapi: fixtures.join("orders.yaml"),
            behaviors: vec![fixtures.join("orders.hcl")],
            artifact: None,
        })
        .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(report.service, "Orders");
        assert_eq!(report.openapi_version, "3.2.1");
        assert_eq!(report.operations, 1);
        assert_eq!(report.hcl_behaviors, 1);
        assert_eq!(report.openapi_defaults, 0);
        assert_eq!(report.async_operations, 1);
        assert_eq!(report.resources, 1);
        assert_eq!(report.artifact, None);
    }

    #[test]
    fn writes_a_loadable_artifact() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let artifact = std::env::temp_dir().join(format!(
            "simile-core-compile-{}.similec",
            std::process::id()
        ));
        drop(fs::remove_file(&artifact));

        let report = compile(&CompileRequest {
            openapi: fixtures.join("orders.yaml"),
            behaviors: vec![fixtures.join("orders.hcl")],
            artifact: Some(artifact.clone()),
        })
        .unwrap_or_else(|error| panic!("{error}"));
        let bytes = fs::read(&artifact).unwrap_or_else(|error| panic!("{error}"));
        let decoded = simile_artifact::decode(&bytes).unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(decoded.summary().service, "Orders");
        assert_eq!(
            report.artifact.as_ref().map(|report| report.path.as_path()),
            Some(artifact.as_path())
        );
        fs::remove_file(&artifact).unwrap_or_else(|error| panic!("{error}"));
    }
}
