use serde::Serialize;

use crate::Context;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    Fail,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub status: CheckStatus,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub ok: bool,
    pub checks: Vec<Check>,
}

/// Inspect the process environment without changing it.
#[must_use]
pub fn doctor(ctx: &Context) -> DoctorReport {
    let directory_exists = ctx.current_dir().is_dir();
    let checks = vec![
        Check {
            name: "build",
            status: CheckStatus::Pass,
            detail: format!("simile {}", ctx.build().version),
        },
        Check {
            name: "current-directory",
            status: if directory_exists {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            detail: ctx.current_dir().display().to_string(),
        },
    ];

    DoctorReport {
        ok: checks.iter().all(|check| check.status == CheckStatus::Pass),
        checks,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::BuildInfo;

    use super::*;

    #[test]
    fn reports_a_valid_current_directory() {
        let build = BuildInfo::new("0.1.0", None::<String>, "x86_64", "linux");
        let ctx = Context::new(build, PathBuf::from(env!("CARGO_MANIFEST_DIR")));

        let report = doctor(&ctx);

        assert!(report.ok);
        assert_eq!(report.checks.len(), 2);
    }

    #[test]
    fn reports_a_missing_current_directory() {
        let build = BuildInfo::new("0.1.0", None::<String>, "x86_64", "linux");
        let ctx = Context::new(build, PathBuf::from("/simile/path/that/does/not/exist"));

        let report = doctor(&ctx);

        assert!(!report.ok);
        assert_eq!(report.checks[1].status, CheckStatus::Fail);
    }
}
