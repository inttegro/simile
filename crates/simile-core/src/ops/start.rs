use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use serde::Serialize;
use simile_artifact::Artifact;
use simile_compiler::{Compiler, ParsedSourceCompileRequest};
use simile_hcl::{ContractDeclaration, ServiceDocument, SourceDeclaration};
use simile_heuristics::Policy;
use simile_spec::{DescriptionEntry, DescriptionKind};

use super::compile::{ContractFormat, LoadedContract, compiler_source, load_contract};
use super::run::{SpecSource, read_spec};
use crate::registry::{Registry, SourceLocation};
use crate::service_config::{LoadedServiceConfig, ServiceConfigSource};
use crate::{Context, Error, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartRequest {
    pub service: Option<String>,
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
    pub backends: usize,
    pub async_operations: usize,
    pub faults: usize,
    pub fault_profiles: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_fault_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fault_seed: Option<u64>,
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
    match (&request.service, &request.config) {
        (requested, Some(config)) => {
            if let Some(service) = requested {
                crate::registry::validate_service_name(service)?;
            }
            let loaded = LoadedServiceConfig::load(ctx, ServiceConfigSource::Path(config.clone()))?;
            Ok(prepare_loaded_service(requested.as_deref(), request.port, &loaded)?.service)
        }
        (Some(service), None) => {
            crate::registry::validate_service_name(service)?;
            prepare_registered_service(ctx, service, request.port)
        }
        (None, None) => {
            let loaded = LoadedServiceConfig::load_simfile(ctx)?;
            Ok(prepare_loaded_service(None, request.port, &loaded)?.service)
        }
    }
}

pub(crate) fn prepare_loaded_service(
    requested_service: Option<&str>,
    port: Option<u16>,
    config: &LoadedServiceConfig,
) -> Result<PreparedConfiguration> {
    let document =
        simile_hcl::parse_service(simile_hcl::Source::new(&config.name, &config.contents))?;
    crate::registry::validate_service_name(&document.service.name)?;
    if let Some(requested_service) = requested_service {
        verify_service_name(requested_service, &document)?;
    }

    let (declaration, format) = match &document.service.contract {
        ContractDeclaration::OpenApi(source) => (source, ContractFormat::OpenApi),
        ContractDeclaration::Smithy(source) => (source, ContractFormat::Smithy),
        ContractDeclaration::TypeScript(source) => (source, ContractFormat::TypeScript),
    };
    let spec = match declaration {
        SourceDeclaration::Path(path) => SpecSource::Path(config.resolve(path, "contract")?),
        SourceDeclaration::Url(url) => SpecSource::Url(url.clone()),
    };
    let openapi_source = match &spec {
        SpecSource::Path(path) => SourceLocation::Path {
            value: path.to_string_lossy().into_owned(),
        },
        SpecSource::Url(url) => SourceLocation::Url { value: url.clone() },
    };
    let contract = match &spec {
        SpecSource::Path(path) => load_contract(path, format)?,
        SpecSource::Url(_) => {
            let (name, contents) = read_spec(&spec)?;
            LoadedContract {
                format,
                sources: vec![(name, contents)],
            }
        }
    };
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

    let mut compiled = Compiler::new().compile_source_parsed(ParsedSourceCompileRequest {
        contract: compiler_source(&contract),
        behaviors,
        heuristic_policy: Policy::Standard,
    })?;
    compiled.principal_setups = simile_hcl::compile_principal_setups(
        document.principal_setups,
        &compiled.contract,
        &compiled.principals,
    )?;
    if let Some(description) = &document.service.description {
        compiled.descriptions.insert(DescriptionEntry {
            kind: DescriptionKind::Service,
            parent: None,
            name: document.service.name.clone(),
            text: description.clone(),
        });
    }
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
        backends: summary.resources,
        async_operations: summary.async_operations,
        faults: summary.faults,
        fault_profiles: summary.fault_profiles,
        active_fault_profile: None,
        fault_seed: None,
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

fn prepare_registered_service(
    ctx: &Context,
    service: &str,
    port: Option<u16>,
) -> Result<PreparedService> {
    let registered = Registry::new(ctx.simile_home()?).load(service)?;
    let summary = registered.artifact.summary();
    let listen = SocketAddr::new(
        registered.manifest.listen.host,
        port.unwrap_or(registered.manifest.listen.port),
    );
    let report = ServiceReport {
        service: registered.manifest.name,
        service_version: summary.service_version,
        openapi_version: summary.openapi_version,
        listen,
        operations: summary.operations,
        backends: summary.resources,
        async_operations: summary.async_operations,
        faults: summary.faults,
        fault_profiles: summary.fault_profiles,
        active_fault_profile: None,
        fault_seed: None,
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
    module.backends.is_empty()
        && module.behaviors.is_empty()
        && module.error_sets.is_empty()
        && module.error_set_applications.is_empty()
        && module.protocol_errors.is_empty()
        && module.scenarios.is_empty()
        && module.fault_uses.is_empty()
        && module.faults.is_empty()
        && module.fault_profiles.is_empty()
        && module.async_operations.is_empty()
        && module.effects.is_empty()
        && module.webhooks.is_empty()
        && module.webhook_policies.is_empty()
        && module.principals.is_empty()
        && module.profiles.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

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

    fn simfile_context() -> (Context, PathBuf) {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "simile-current-simfile-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        drop(fs::remove_dir_all(&root));
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("{error}"));
        for (source, destination) in [
            ("service-orders.hcl", "Simfile"),
            ("run-orders.yaml", "run-orders.yaml"),
            ("orders.hcl", "orders.hcl"),
        ] {
            fs::copy(fixture(source), root.join(destination))
                .unwrap_or_else(|error| panic!("{error}"));
        }
        let context = Context::with_simile_home(
            crate::BuildInfo::new("0.1.0", None::<String>, "test", "test"),
            &root,
            root.join(".simile"),
        );
        (context, root)
    }

    #[test]
    fn prepares_the_current_simfile_without_a_repeated_service_name() {
        let (context, root) = simfile_context();
        let prepared = prepare_service(
            &context,
            &StartRequest {
                service: None,
                config: None,
                port: Some(8484),
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(prepared.report.service, "orders");
        assert_eq!(prepared.report.listen.port(), 8484);
        assert_eq!(prepared.report.operations, 3);

        fs::remove_dir_all(root).unwrap_or_else(|error| panic!("{error}"));
    }

    #[test]
    fn prepares_a_service_and_applies_the_port_override() {
        let prepared = prepare_service(
            &context(),
            &StartRequest {
                service: Some("orders".to_owned()),
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
        assert_eq!(prepared.report.backends, 1);
        assert_eq!(prepared.report.async_operations, 1);
        assert_eq!(prepared.artifact.operations().count(), 3);
    }

    #[test]
    fn rejects_a_requested_name_that_differs_from_the_configuration() {
        let result = prepare_service(
            &context(),
            &StartRequest {
                service: Some("payments".to_owned()),
                config: Some(fixture("service-orders.hcl")),
                port: None,
            },
        );

        assert!(matches!(result, Err(Error::ServiceNameMismatch { .. })));
    }
}
