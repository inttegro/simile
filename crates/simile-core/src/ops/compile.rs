use std::fs;
use std::path::PathBuf;

use serde::Serialize;
use simile_compiler::{Compiler, Source};
use simile_spec::BehaviorOrigin;

use crate::{Error, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompileRequest {
    pub openapi: PathBuf,
    pub behaviors: Vec<PathBuf>,
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
}

/// Compile an OpenAPI contract and zero or more HCL behavior modules.
pub fn compile(request: &CompileRequest) -> Result<CompileReport> {
    let openapi = read(&request.openapi)?;
    let behavior_contents = request
        .behaviors
        .iter()
        .map(read)
        .collect::<Result<Vec<_>>>()?;
    let behavior_names = request
        .behaviors
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let behaviors = behavior_names
        .iter()
        .zip(&behavior_contents)
        .map(|(name, contents)| Source::new(name, contents))
        .collect();
    let openapi_name = request.openapi.to_string_lossy().into_owned();
    let compiled = Compiler::new().compile(simile_compiler::CompileRequest {
        openapi: Source::new(&openapi_name, &openapi),
        behaviors,
    })?;
    let hcl_behaviors = compiled
        .operations
        .values()
        .filter(|operation| operation.origin == BehaviorOrigin::Hcl)
        .count();

    Ok(CompileReport {
        service: compiled.contract.info.title,
        service_version: compiled.contract.info.version,
        openapi_version: compiled.contract.openapi.exact,
        operations: compiled.operations.len(),
        openapi_defaults: compiled.operations.len() - hcl_behaviors,
        hcl_behaviors,
        async_operations: compiled.async_operations.len(),
        resources: compiled.resources.len(),
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
        })
        .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(report.service, "Orders");
        assert_eq!(report.openapi_version, "3.2.1");
        assert_eq!(report.operations, 1);
        assert_eq!(report.hcl_behaviors, 1);
        assert_eq!(report.openapi_defaults, 0);
        assert_eq!(report.async_operations, 1);
        assert_eq!(report.resources, 1);
    }
}
