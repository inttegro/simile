//! Built-in environmental faults that service authors can import by name.
//!
//! Catalog entries are ordinary [`Fault`] values. Source adapters may offer
//! concise import syntax and controlled overrides, but compiled services never
//! retain a dependency on this catalog. This keeps artifacts self-contained
//! and makes a catalog import equivalent to an explicit provider declaration.

use crate::{Fault, FaultAction, FaultPhase};

/// A delayed response with a one-second default latency.
pub const HTTP_SLOW: &str = "http.slow";

/// A committed operation whose response connection is closed before delivery.
pub const HTTP_RESPONSE_LOST: &str = "http.response_lost";

/// Default latency for [`HTTP_SLOW`].
pub const HTTP_SLOW_MILLISECONDS: u64 = 1_000;

const NAMES: &[&str] = &[HTTP_RESPONSE_LOST, HTTP_SLOW];

/// Return the stable names of every fault included with this Simile release.
#[must_use]
pub const fn names() -> &'static [&'static str] {
    NAMES
}

/// Return the public human-readable explanation bundled with a catalog fault.
#[must_use]
pub fn description(name: &str) -> Option<&'static str> {
    match name {
        HTTP_SLOW => Some("Delay the completed response before delivery."),
        HTTP_RESPONSE_LOST => Some(
            "Commit the operation, then close delivery before the caller receives the response.",
        ),
        _ => None,
    }
}

/// Resolve one catalog entry into its complete canonical fault definition.
///
/// The returned value is owned so an authoring adapter can apply a supported
/// override without mutating the process-wide preset.
#[must_use]
pub fn get(name: &str) -> Option<Fault> {
    let (phase, action) = match name {
        HTTP_SLOW => (
            FaultPhase::BeforeResponse,
            FaultAction::Delay {
                milliseconds: HTTP_SLOW_MILLISECONDS,
            },
        ),
        HTTP_RESPONSE_LOST => (FaultPhase::AfterCommit, FaultAction::Disconnect),
        _ => return None,
    };
    Some(Fault {
        name: name.to_owned(),
        phase,
        action,
    })
}

/// Expand an exact name, namespace wildcard, or whole-catalog wildcard.
///
/// Supported selectors are `http.slow`, `http.*`, and `*`. Exclusion is an
/// authoring-language concern; callers strip a leading `^` before expansion.
#[must_use]
pub fn select(selector: &str) -> Vec<Fault> {
    if selector == "*" {
        return names().iter().filter_map(|name| get(name)).collect();
    }
    if let Some(namespace) = selector.strip_suffix(".*") {
        let prefix = format!("{namespace}.");
        return names()
            .iter()
            .filter(|name| name.starts_with(&prefix))
            .filter_map(|name| get(name))
            .collect();
    }
    get(selector).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_complete_http_faults() {
        let slow = get(HTTP_SLOW).unwrap_or_else(|| panic!("{HTTP_SLOW} must be present"));
        assert_eq!(slow.phase, FaultPhase::BeforeResponse);
        assert_eq!(
            slow.action,
            FaultAction::Delay {
                milliseconds: 1_000
            }
        );

        let lost = get(HTTP_RESPONSE_LOST)
            .unwrap_or_else(|| panic!("{HTTP_RESPONSE_LOST} must be present"));
        assert_eq!(lost.phase, FaultPhase::AfterCommit);
        assert_eq!(lost.action, FaultAction::Disconnect);
    }

    #[test]
    fn rejects_unknown_catalog_names() {
        assert_eq!(get("http.not_real"), None);
    }

    #[test]
    fn selects_exact_namespace_and_complete_catalogs() {
        assert_eq!(select(HTTP_SLOW).len(), 1);
        assert_eq!(select("http.*").len(), 2);
        assert_eq!(select("*").len(), names().len());
        assert!(select("http2.*").is_empty());
    }
}
