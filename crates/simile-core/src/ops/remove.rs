use std::path::PathBuf;

use serde::Serialize;

use crate::registry::Registry;
use crate::{Context, Error, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoveRequest {
    pub service: String,
    pub confirm: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveReport {
    pub service: String,
    pub directory: PathBuf,
}

/// Permanently remove one named service after exact-name confirmation.
pub fn remove(ctx: &Context, request: &RemoveRequest) -> Result<RemoveReport> {
    if request.confirm != request.service {
        return Err(Error::ServiceRemovalNotConfirmed {
            service: request.service.clone(),
        });
    }

    let registry = Registry::new(ctx.simile_home()?);
    let directory = registry.remove(&request.service)?;
    Ok(RemoveReport {
        service: request.service.clone(),
        directory,
    })
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

    fn test_context() -> (Context, PathBuf) {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "simile-remove-test-{}-{}",
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
    fn requires_an_exact_confirmation_before_removing_the_service() {
        let (ctx, root) = test_context();
        register(
            &ctx,
            &RegisterRequest {
                service: "orders".to_owned(),
                config: fixture("service-orders.hcl").to_string_lossy().into_owned(),
                force: false,
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));

        let result = remove(
            &ctx,
            &RemoveRequest {
                service: "orders".to_owned(),
                confirm: "payments".to_owned(),
            },
        );

        assert!(matches!(
            result,
            Err(Error::ServiceRemovalNotConfirmed { .. })
        ));
        assert!(root.join("services/orders/current").is_file());
        fs::remove_dir_all(root).unwrap_or_else(|error| panic!("{error}"));
    }

    #[test]
    fn removes_only_the_confirmed_service_directory() {
        let (ctx, root) = test_context();
        for service in ["orders", "payments"] {
            let inputs = root.join("inputs").join(service);
            fs::create_dir_all(&inputs).unwrap_or_else(|error| panic!("{error}"));
            for name in ["run-orders.yaml", "orders.hcl"] {
                fs::copy(fixture(name), inputs.join(name))
                    .unwrap_or_else(|error| panic!("{error}"));
            }
            let config = fs::read_to_string(fixture("service-orders.hcl"))
                .unwrap_or_else(|error| panic!("{error}"))
                .replace("service \"orders\"", &format!("service \"{service}\""));
            let config_path = inputs.join("service.hcl");
            fs::write(&config_path, config).unwrap_or_else(|error| panic!("{error}"));
            register(
                &ctx,
                &RegisterRequest {
                    service: service.to_owned(),
                    config: config_path.to_string_lossy().into_owned(),
                    force: false,
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        }
        fs::write(root.join("services/orders/state/resource.json"), "{}")
            .unwrap_or_else(|error| panic!("{error}"));

        let report = remove(
            &ctx,
            &RemoveRequest {
                service: "orders".to_owned(),
                confirm: "orders".to_owned(),
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(report.service, "orders");
        assert_eq!(report.directory, root.join("services/orders"));
        assert!(!report.directory.exists());
        assert!(root.join("services/payments/current").is_file());
        assert!(root.join("layout-version").is_file());
        fs::remove_dir_all(root).unwrap_or_else(|error| panic!("{error}"));
    }
}
