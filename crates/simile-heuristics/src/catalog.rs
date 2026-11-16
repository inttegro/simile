use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::model::{Application, Policy, Rule};
use crate::{accounting, apply, collections, temporal};

/// A provider-neutral synthesis plan compiled from one OpenAPI schema object.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Plan(Vec<Rule>);

impl Plan {
    /// Compile cautious hints from exact field names and compatible OpenAPI types.
    #[must_use]
    pub fn compile(schema: &Map<String, Value>, policy: Policy) -> Self {
        if policy == Policy::Off {
            return Self::default();
        }
        let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
            return Self::default();
        };

        let explicit = explicit_paths(schema.get("x-simile-constraints"));
        let mut rules = Vec::new();
        rules.extend(temporal::detect(properties));
        rules.extend(collections::detect(properties));
        rules.extend(accounting::detect(properties));
        rules.retain(|rule| policy.admits(rule.confidence) && !overridden(rule, &explicit));
        rules.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.evidence.paths().cmp(right.evidence.paths()))
        });
        rules.dedup();

        Self(rules)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn rules(&self) -> impl Iterator<Item = &Rule> {
        self.0.iter()
    }

    /// Apply the compiled hints to a synthesized object and report every repair.
    ///
    /// This method never decides whether a payload is valid. Its caller must validate the
    /// resulting candidate against the authoritative schema and roll back on failure.
    pub fn apply(&self, value: &mut Value) -> Vec<Application> {
        let Some(object) = value.as_object_mut() else {
            return Vec::new();
        };

        self.0
            .iter()
            .filter_map(|rule| apply::rule(rule, object))
            .collect()
    }

    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        let mut rules = BTreeSet::new();
        self.0.iter().all(|rule| {
            !rule.id.is_empty()
                && !rule.evidence.paths().is_empty()
                && rules.insert((rule.id.as_str(), rule.evidence.paths()))
        })
    }
}

fn overridden(rule: &Rule, explicit: &BTreeSet<String>) -> bool {
    rule.evidence
        .paths()
        .iter()
        .any(|path| explicit.iter().any(|declared| overlaps(path, declared)))
}

fn overlaps(left: &str, right: &str) -> bool {
    left == right
        || left
            .strip_prefix(right)
            .is_some_and(|suffix| suffix.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn explicit_paths(value: Option<&Value>) -> BTreeSet<String> {
    fn visit(value: &Value, paths: &mut BTreeSet<String>) {
        match value {
            Value::String(value) if value.starts_with('/') => {
                paths.insert(value.clone());
            }
            Value::Array(values) => values.iter().for_each(|value| visit(value, paths)),
            Value::Object(values) => values.values().for_each(|value| visit(value, paths)),
            _ => {}
        }
    }

    let mut paths = BTreeSet::new();
    if let Some(value) = value {
        visit(value, &mut paths);
    }
    paths
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn explicit_constraints_suppress_overlapping_hints() {
        let schema = json!({
            "type": "object",
            "properties": {
                "created_at": {"type": "integer"},
                "updated_at": {"type": "integer"}
            },
            "x-simile-constraints": [{
                "ordered": ["/created_at", "/updated_at"]
            }]
        });

        let plan = Plan::compile(
            schema.as_object().unwrap_or_else(|| panic!("object")),
            Policy::Aggressive,
        );

        assert!(plan.is_empty());
    }

    #[test]
    fn policies_admit_increasing_confidence_bands() {
        let schema = json!({
            "type": "object",
            "properties": {
                "created_at": {"type": "integer"},
                "updated_at": {"type": "integer"},
                "items": {"type": "array", "items": {"type": "string"}},
                "item_count": {"type": "integer"},
                "amount_due": {"type": "integer"},
                "amount_paid": {"type": "integer"},
                "amount_remaining": {"type": "integer"}
            }
        });
        let schema = schema.as_object().unwrap_or_else(|| panic!("object"));

        assert_eq!(Plan::compile(schema, Policy::Off).len(), 0);
        assert_eq!(Plan::compile(schema, Policy::Conservative).len(), 1);
        assert_eq!(Plan::compile(schema, Policy::Standard).len(), 2);
        assert_eq!(Plan::compile(schema, Policy::Aggressive).len(), 3);
    }

    #[test]
    fn applies_temporal_rules_and_reports_changes() {
        let schema = json!({
            "type": "object",
            "properties": {
                "created_at": {"type": "string", "format": "date-time"},
                "updated_at": {"type": "string", "format": "date-time"}
            }
        });
        let plan = Plan::compile(
            schema.as_object().unwrap_or_else(|| panic!("object")),
            Policy::Conservative,
        );
        let mut value = json!({
            "created_at": "2026-11-16T10:00:00Z",
            "updated_at": "2025-11-16T10:00:00Z"
        });

        let applied = plan.apply(&mut value);

        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].rule_id, "generic.temporal.created-updated.v1");
        assert_eq!(value["updated_at"], value["created_at"]);
    }
}
