use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use serde::Serialize;
use simile_artifact::Artifact;
use simile_compiler::{Compiler, ParsedCompileRequest, Source};
use simile_hcl::{OpenApiDeclaration, ServiceDocument};
use simile_heuristics::Policy;

use super::run::{SpecSource, read_spec};
use crate::registry::{Registry, SourceLocation};
use crate::service_config::{LoadedServiceConfig, ServiceConfigSource};
use crate::{Context, Error, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartRequest {
    pub service: String,
    pub config: Option<PathBuf>,
    pub port: Option<u16>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceReport {
    pub service: String,
    pub service_version: String,
    pub openapi_version: String,
    pub listen: SocketAddr,
    pub operations: usize,
    pub resources: usize,
    pub async_operations: usize,
}

#[derive(Clone, Debug)]
pub struct PreparedService {
    pub report: ServiceReport,
    pub artifact: Artifact,
    pub(crate) artifact_bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedConfiguration {
    pub service: PreparedService,
    pub openapi_source: SourceLocation,
}

/// Resolve, validate, and compile a runnable service without binding its socket.
pub fn prepare_service(ctx: &Context, request: &StartRequest) -> Result<PreparedService> {
    match &request.config {
        Some(config) => {
            let loaded = LoadedServiceConfig::load(ctx, ServiceConfigSource::Path(config.clone()))?;
            Ok(prepare_loaded_service(&request.service, request.port, &loaded)?.service)
        }
        None => prepare_registered_service(ctx, request),
    }
}

pub(crate) fn prepare_loaded_service(
    requested_service: &str,
    port: Option<u16>,
    config: &LoadedServiceConfig,
) -> Result<PreparedConfiguration> {
    let document =
        simile_hcl::parse_service(simile_hcl::Source::new(&config.name, &config.contents))?;
    verify_service_name(requested_service, &document)?;

    let spec = match &document.service.openapi {
        OpenApiDeclaration::Path(path) => SpecSource::Path(config.resolve(path, "OpenAPI")?),
        OpenApiDeclaration::Url(url) => SpecSource::Url(url.clone()),
    };
    let openapi_source = match &spec {
        SpecSource::Path(path) => SourceLocation::Path {
            value: path.to_string_lossy().into_owned(),
        },
        SpecSource::Url(url) => SourceLocation::Url { value: url.clone() },
    };
    let (openapi_name, openapi) = read_spec(&spec)?;
    let mut behaviors = Vec::with_capacity(document.service.behaviors.len() + 1);
    if !module_is_empty(&document.inline_behavior) {
        behaviors.push(document.inline_behavior);
    }
    for behavior in &document.service.behaviors {
        let path = config.resolve(behavior, "behavior")?;
        let contents = std::fs::read_to_string(&path).map_err(|source| Error::ReadSource {
            path: path.clone(),
            source,
        })?;
        behaviors.push(simile_hcl::parse(simile_hcl::Source::new(
            &path.to_string_lossy(),
            &contents,
        ))?);
    }

    let compiled = Compiler::new().compile_parsed(ParsedCompileRequest {
        openapi: Source::new(&openapi_name, &openapi),
        behaviors,
        heuristic_policy: Policy::Standard,
    })?;
    let encoded = simile_artifact::encode(&compiled)?;
    let artifact = simile_artifact::decode(&encoded)?;
    let host = document
        .service
        .listen
        .host
        .parse::<IpAddr>()
        .map_err(|source| Error::InvalidListenHost {
            host: document.service.listen.host.clone(),
            source,
        })?;
    let listen = SocketAddr::new(host, port.unwrap_or(document.service.listen.port));
    let summary = artifact.summary();
    let report = ServiceReport {
        service: document.service.name,
        service_version: summary.service_version,
        openapi_version: summary.openapi_version,
        listen,
        operations: summary.operations,
        resources: summary.resources,
        async_operations: summary.async_operations,
    };

    Ok(PreparedConfiguration {
        service: PreparedService {
            report,
            artifact,
            artifact_bytes: encoded,
        },
        openapi_source,
    })
}

fn prepare_registered_service(ctx: &Context, request: &StartRequest) -> Result<PreparedService> {
    let registered = Registry::new(ctx.simile_home()?).load(&request.service)?;
    let summary = registered.artifact.summary();
    let listen = SocketAddr::new(
        registered.manifest.listen.host,
        request.port.unwrap_or(registered.manifest.listen.port),
    );
    let report = ServiceReport {
        service: registered.manifest.name,
        service_version: summary.service_version,
        openapi_version: summary.openapi_version,
        listen,
        operations: summary.operations,
        resources: summary.resources,
        async_operations: summary.async_operations,
    };
    Ok(PreparedService {
        report,
        artifact: registered.artifact,
        artifact_bytes: registered.artifact_bytes,
    })
}

fn verify_service_name(requested: &str, document: &ServiceDocument) -> Result<()> {
    if requested == document.service.name {
        return Ok(());
    }
    Err(Error::ServiceNameMismatch {
        requested: requested.to_owned(),
        configured: document.service.name.clone(),
    })
}

fn module_is_empty(module: &simile_hcl::Module) -> bool {
    module.resources.is_empty() && module.behaviors.is_empty() && module.async_operations.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn context() -> Context {
        Context::with_simile_home(
            crate::BuildInfo::new("0.1.0", None::<String>, "test", "test"),
            env!("CARGO_MANIFEST_DIR"),
            Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-simile-home"),
        )
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn prepares_a_service_and_applies_the_port_override() {
        let prepared = prepare_service(
            &context(),
            &StartRequest {
                service: "orders".to_owned(),
                config: Some(fixture("service-orders.hcl")),
                port: Some(8484),
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(prepared.report.service, "orders");
        assert_eq!(
            prepared.report.listen,
            SocketAddr::from(([127, 0, 0, 1], 8484))
        );
        assert_eq!(prepared.report.operations, 3);
        assert_eq!(prepared.report.resources, 1);
        assert_eq!(prepared.report.async_operations, 1);
        assert_eq!(prepared.artifact.operations().count(), 3);
    }

    #[test]
    fn rejects_a_requested_name_that_differs_from_the_configuration() {
        let result = prepare_service(
            &context(),
            &StartRequest {
                service: "payments".to_owned(),
                config: Some(fixture("service-orders.hcl")),
                port: None,
            },
        );

        assert!(matches!(result, Err(Error::ServiceNameMismatch { .. })));
    }
}
