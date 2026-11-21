use std::path::PathBuf;

use super::start::{PreparedConfiguration, prepare_loaded_service};
use crate::registry::{RegistrationInput, Registry};
use crate::service_config::{LoadedServiceConfig, ServiceConfigSource};
use crate::{Context, Result};
use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisterRequest {
    pub service: Option<String>,
    pub config: Option<String>,
    pub force: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationReport {
    pub service: String,
    pub revision: String,
    pub service_version: String,
    pub openapi_version: String,
    pub listen: std::net::SocketAddr,
    pub operations: usize,
    pub backends: usize,
    pub async_operations: usize,
    pub directory: PathBuf,
    pub config: PathBuf,
    pub artifact: PathBuf,
}

/// Compile and atomically install one named service into the local registry.
pub fn register(ctx: &Context, request: &RegisterRequest) -> Result<RegistrationReport> {
    let registry = Registry::new(ctx.simile_home()?);
    if let Some(service) = &request.service {
        registry.ensure_registration_available(service, request.force)?;
    }
    let config = match &request.config {
        Some(config) => {
            LoadedServiceConfig::load(ctx, ServiceConfigSource::from_argument(config.clone()))?
        }
        None => LoadedServiceConfig::load_simfile(ctx)?,
    };
    let configured = prepare_loaded_service(request.service.as_deref(), None, None, &config)?;
    let service = configured.service.report.service.clone();
    if request.service.is_none() {
        registry.ensure_registration_available(&service, request.force)?;
    }
    install(&registry, &service, &config, configured, request.force)
}

pub(crate) fn install(
    registry: &Registry,
    service: &str,
    config: &LoadedServiceConfig,
    configured: PreparedConfiguration,
    replace: bool,
) -> Result<RegistrationReport> {
    let prepared = configured.service;
    let registered = registry.register(RegistrationInput {
        service,
        config: &config.contents,
        config_source: config.source.clone(),
        openapi_source: configured.openapi_source,
        listen: prepared.report.listen,
        artifact: &prepared.artifact,
        artifact_bytes: &prepared.artifact_bytes,
        replace,
    })?;
    let summary = registered.artifact.summary();

    Ok(RegistrationReport {
        service: registered.manifest.name,
        revision: registered.revision,
        service_version: summary.service_version,
        openapi_version: summary.openapi_version,
        listen: prepared.report.listen,
        operations: summary.operations,
        backends: summary.resources,
        async_operations: summary.async_operations,
        config: registered.directory.join(&registered.manifest.config),
        artifact: registered.directory.join(&registered.manifest.artifact),
        directory: registered.directory,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::ops::{StartRequest, prepare_service};
    use crate::{BuildInfo, Error};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn test_context() -> (Context, PathBuf) {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);

        let root = std::env::temp_dir().join(format!(
            "simile-register-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        drop(fs::remove_dir_all(&root));
        let context = Context::with_simile_home(
            BuildInfo::new("0.1.0", None::<String>, "test", "test"),
            env!("CARGO_MANIFEST_DIR"),
            &root,
        );
        (context, root)
    }

    #[test]
    fn registers_and_loads_an_immutable_service_revision() {
        let (ctx, root) = test_context();
        let inputs = root.with_extension("inputs");
        fs::create_dir_all(&inputs).unwrap_or_else(|error| panic!("{error}"));
        for name in ["service-orders.hcl", "run-orders.yaml", "orders.hcl"] {
            fs::copy(fixture(name), inputs.join(name)).unwrap_or_else(|error| panic!("{error}"));
        }
        let report = register(
            &ctx,
            &RegisterRequest {
                service: Some("orders".to_owned()),
                config: Some(
                    inputs
                        .join("service-orders.hcl")
                        .to_string_lossy()
                        .into_owned(),
                ),
                force: false,
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));

        assert!(report.config.is_file());
        assert!(report.artifact.is_file());
        assert!(report.directory.join("manifest.json").is_file());
        assert_eq!(
            fs::read_to_string(root.join("services/orders/current"))
                .unwrap_or_else(|error| panic!("{error}"))
                .trim(),
            report.revision
        );

        fs::remove_dir_all(&inputs).unwrap_or_else(|error| panic!("{error}"));

        let prepared = prepare_service(
            &ctx,
            &StartRequest {
                service: Some("orders".to_owned()),
                config: None,
                host: None,
                port: Some(8484),
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(prepared.report.service, "orders");
        assert_eq!(prepared.report.listen.port(), 8484);
        assert_eq!(prepared.artifact.operations().count(), 3);

        let duplicate = register(
            &ctx,
            &RegisterRequest {
                service: Some("orders".to_owned()),
                config: Some(
                    inputs
                        .join("service-orders.hcl")
                        .to_string_lossy()
                        .into_owned(),
                ),
                force: false,
            },
        );
        assert!(matches!(
            duplicate,
            Err(Error::ServiceAlreadyRegistered { .. })
        ));

        fs::remove_dir_all(root).unwrap_or_else(|error| panic!("{error}"));
    }
}
