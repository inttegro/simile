use serde_json::{Map, Value};

use crate::model::{Action, Confidence, Evidence, Rule};
use crate::schema::{comparable, pointer};

const CREATED_UPDATED: &str = "generic.temporal.created-updated.v1";
const START_END: &str = "generic.temporal.start-end.v1";
const NAMED_RANGE: &str = "generic.temporal.named-range.v1";

/// Detects temporal ordering relationships whose field names and schema types agree.
///
/// For example, compatible `created_at` and `updated_at` date-time fields imply
/// `created_at <= updated_at`. A numeric `period_start` and `period_end` pair implies the
/// equivalent numeric ordering. A pair is ignored when its OpenAPI types are incompatible.
pub(crate) fn detect(properties: &Map<String, Value>) -> Vec<Rule> {
    let mut rules = Vec::new();
    push_order_rule(
        &mut rules,
        properties,
        CREATED_UPDATED,
        ("created_at", "updated_at"),
    );
    push_order_rule(&mut rules, properties, START_END, ("start", "end"));

    for earlier in properties.keys() {
        let Some(prefix) = earlier.strip_suffix("_start") else {
            continue;
        };
        let later = format!("{prefix}_end");
        push_order_rule(&mut rules, properties, NAMED_RANGE, (earlier, &later));
    }

    rules
}

fn push_order_rule(
    rules: &mut Vec<Rule>,
    properties: &Map<String, Value>,
    id: &str,
    fields: (&str, &str),
) {
    let (earlier, later) = fields;
    let Some(left) = properties.get(earlier) else {
        return;
    };
    let Some(right) = properties.get(later) else {
        return;
    };
    if comparable(left) != comparable(right) || comparable(left).is_none() {
        return;
    }

    rules.push(Rule {
        id: id.to_owned(),
        confidence: Confidence::High,
        evidence: Evidence::ExactFields(vec![pointer(earlier), pointer(later)]),
        action: Action::Ordered {
            earlier: earlier.to_owned(),
            later: later.to_owned(),
        },
    });
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn detects_only_compatible_temporal_pairs() {
        let schema = json!({
            "created_at": {"type": "string", "format": "date-time"},
            "updated_at": {"type": "string", "format": "date-time"},
            "start": {"type": "integer"},
            "end": {"type": "string"},
            "period_start": {"type": "integer"},
            "period_end": {"type": "integer"}
        });

        let rules = detect(schema.as_object().unwrap_or_else(|| panic!("object")));

        assert_eq!(rules.len(), 2);
        assert!(rules.iter().any(|rule| rule.id == CREATED_UPDATED));
        assert!(rules.iter().any(|rule| rule.id == NAMED_RANGE));
    }
}
