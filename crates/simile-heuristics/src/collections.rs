use serde_json::{Map, Value};

use crate::model::{Action, Confidence, Evidence, Rule};
use crate::schema::{is_array, is_integer, pointer};

const COUNT: &str = "generic.collection.count.v1";

/// Detects exact collection/count pairs while avoiding list-pagination envelopes.
///
/// For example, `items: array` and `item_count: integer` imply that the count should equal
/// `items.len()`. An object containing `has_more`, `next`, `next_page`, or `total_count` is
/// treated as a pagination envelope and is left alone because its counts may describe a page
/// or the full result set rather than the embedded array.
pub(crate) fn detect(properties: &Map<String, Value>) -> Vec<Rule> {
    if has_pagination_markers(properties) {
        return Vec::new();
    }

    properties
        .iter()
        .filter_map(|(collection, schema)| {
            if !is_array(schema) {
                return None;
            }
            let singular = singular(collection)?;
            let count = format!("{singular}_count");
            properties
                .get(&count)
                .is_some_and(is_integer)
                .then(|| Rule {
                    id: COUNT.to_owned(),
                    confidence: Confidence::Medium,
                    evidence: Evidence::ExactFields(vec![pointer(collection), pointer(&count)]),
                    action: Action::Count {
                        collection: collection.clone(),
                        count,
                    },
                })
        })
        .collect()
}

fn has_pagination_markers(properties: &Map<String, Value>) -> bool {
    ["has_more", "next", "next_page", "total_count"]
        .iter()
        .any(|name| properties.contains_key(*name))
}

fn singular(value: &str) -> Option<String> {
    value
        .strip_suffix("ies")
        .map(|prefix| format!("{prefix}y"))
        .or_else(|| value.strip_suffix('s').map(str::to_owned))
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn detects_collection_count_pairs() {
        let properties = json!({
            "items": {"type": "array"},
            "item_count": {"type": "integer"}
        });

        let rules = detect(properties.as_object().unwrap_or_else(|| panic!("object")));

        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].id, COUNT);
    }

    #[test]
    fn skips_pagination_envelopes() {
        let properties = json!({
            "items": {"type": "array"},
            "item_count": {"type": "integer"},
            "has_more": {"type": "boolean"}
        });

        let rules = detect(properties.as_object().unwrap_or_else(|| panic!("object")));

        assert!(rules.is_empty());
    }
}
