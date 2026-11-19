use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{ReplayRequest, replay};
use crate::{Error, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConformanceRequest {
    pub bundles: Vec<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConformanceCase {
    pub bundle: PathBuf,
    pub operation: Option<String>,
    pub passed: bool,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConformanceReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub cases: Vec<ConformanceCase>,
}

/// Replay a provider's executable examples as one deterministic conformance suite.
pub fn verify_conformance(request: &ConformanceRequest) -> Result<ConformanceReport> {
    let mut bundles = Vec::new();
    for path in &request.bundles {
        discover(path, &mut bundles)?;
    }
    bundles.sort();
    bundles.dedup();
    if bundles.is_empty() {
        return Err(Error::NoConformanceBundles);
    }
    let cases = bundles
        .into_iter()
        .map(|bundle| {
            match replay(&ReplayRequest {
                bundle: bundle.clone(),
            }) {
                Ok(report) => ConformanceCase {
                    bundle,
                    operation: Some(report.operation),
                    passed: report.matched,
                    message: if report.matched {
                        "response reproduced exactly".to_owned()
                    } else {
                        "response diverged from the recorded expectation".to_owned()
                    },
                },
                Err(error) => ConformanceCase {
                    bundle,
                    operation: None,
                    passed: false,
                    message: error.to_string(),
                },
            }
        })
        .collect::<Vec<_>>();
    let passed = cases.iter().filter(|case| case.passed).count();
    Ok(ConformanceReport {
        total: cases.len(),
        passed,
        failed: cases.len() - passed,
        cases,
    })
}

fn discover(path: &Path, bundles: &mut Vec<PathBuf>) -> Result<()> {
    if path.join("bundle.json").is_file() {
        bundles.push(path.to_path_buf());
        return Ok(());
    }
    if !path.is_dir() {
        return Err(Error::ConformancePath {
            path: path.to_path_buf(),
        });
    }
    for entry in fs::read_dir(path).map_err(|source| Error::ReproductionIo {
        action: "read",
        path: path.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::ReproductionIo {
            action: "read",
            path: path.to_path_buf(),
            source,
        })?;
        let child = entry.path();
        if child.is_dir() && child.join("bundle.json").is_file() {
            bundles.push(child);
        }
    }
    Ok(())
}
