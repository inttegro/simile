use serde::Serialize;

use super::register::{RegistrationReport, install};
use super::start::prepare_loaded_service;
use crate::registry::Registry;
use crate::service_config::{LoadedServiceConfig, ServiceConfigSource};
use crate::{Context, Result};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRequest {
    pub service: String,
    pub config: Option<String>,
}

/// Recompile a registered service and atomically advance its current revision.
pub fn update(ctx: &Context, request: &UpdateRequest) -> Result<RegistrationReport> {
    let registry = Registry::new(ctx.simile_home()?);
    let current = registry.load(&request.service)?;
    let config = match &request.config {
        Some(source) => {
            LoadedServiceConfig::load(ctx, ServiceConfigSource::from_argument(source.clone()))?
        }
        None => {
            LoadedServiceConfig::snapshot(current.manifest.config_source, current.config_contents)
        }
    };
    let configured = prepare_loaded_service(Some(&request.service), None, None, &config)?;
    install(&registry, &request.service, &config, configured, true)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::ops::{RegisterRequest, register};
    use crate::{BuildInfo, Error};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn test_context() -> (Context, PathBuf, PathBuf) {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "simile-update-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        drop(fs::remove_dir_all(&root));
        let inputs = root.join("inputs");
        fs::create_dir_all(&inputs).unwrap_or_else(|error| panic!("{error}"));
        for name in ["service-orders.hcl", "run-orders.yaml", "orders.hcl"] {
            fs::copy(fixture(name), inputs.join(name)).unwrap_or_else(|error| panic!("{error}"));
        }
        let context = Context::with_simile_home(
            BuildInfo::new("0.1.0", None::<String>, "test", "test"),
            env!("CARGO_MANIFEST_DIR"),
            &root,
        );
        (context, root, inputs)
    }

    #[test]
    fn recompiles_the_stored_configuration_into_a_new_revision() {
        let (ctx, root, inputs) = test_context();
        let original = register(
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
        let spec = inputs.join("run-orders.yaml");
        let revised = fs::read_to_string(&spec)
            .unwrap_or_else(|error| panic!("{error}"))
            .replace("version: 1.0.0", "version: 2.0.0");
        fs::write(&spec, revised).unwrap_or_else(|error| panic!("{error}"));

        let updated = update(
            &ctx,
            &UpdateRequest {
                service: "orders".to_owned(),
                config: None,
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));

        assert_ne!(updated.revision, original.revision);
        assert_eq!(updated.service_version, "2.0.0");
        assert!(original.directory.is_dir());
        assert_eq!(
            fs::read_to_string(root.join("services/orders/current"))
                .unwrap_or_else(|error| panic!("{error}"))
                .trim(),
            updated.revision
        );
        fs::remove_dir_all(root).unwrap_or_else(|error| panic!("{error}"));
    }

    #[test]
    fn failed_update_keeps_the_current_revision() {
        let (ctx, root, inputs) = test_context();
        let original = register(
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
        fs::write(inputs.join("run-orders.yaml"), "not: openapi")
            .unwrap_or_else(|error| panic!("{error}"));

        let result = update(
            &ctx,
            &UpdateRequest {
                service: "orders".to_owned(),
                config: None,
            },
        );

        assert!(matches!(result, Err(Error::Compile(_))));
        assert_eq!(
            fs::read_to_string(root.join("services/orders/current"))
                .unwrap_or_else(|error| panic!("{error}"))
                .trim(),
            original.revision
        );
        fs::remove_dir_all(root).unwrap_or_else(|error| panic!("{error}"));
    }
}
