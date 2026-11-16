use serde_json::{Map, Value};

use crate::model::{Action, Confidence, Evidence, Rule};
use crate::schema::{is_number, pointer};

const AMOUNT_REMAINING: &str = "generic.amount.remaining.v1";

/// Detects the narrow `amount_remaining = amount_due - amount_paid` identity.
///
/// This inference is intentionally low confidence: fees, credits, overpayment, tax, or provider
/// accounting rules can make the equation false. It is therefore only admitted by the aggressive
/// policy. For example, fields named `total`, `subtotal`, and `tax` are not inferred at all.
pub(crate) fn detect(properties: &Map<String, Value>) -> Vec<Rule> {
    let fields = ["amount_due", "amount_paid", "amount_remaining"];
    if !fields
        .iter()
        .all(|name| properties.get(*name).is_some_and(is_number))
    {
        return Vec::new();
    }

    vec![Rule {
        id: AMOUNT_REMAINING.to_owned(),
        confidence: Confidence::Low,
        evidence: Evidence::ExactFields(fields.iter().map(|field| pointer(field)).collect()),
        action: Action::Difference {
            minuend: "amount_due".to_owned(),
            subtrahend: "amount_paid".to_owned(),
            result: "amount_remaining".to_owned(),
        },
    }]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn requires_all_three_numeric_fields() {
        let complete = json!({
            "amount_due": {"type": "integer"},
            "amount_paid": {"type": "integer"},
            "amount_remaining": {"type": "integer"}
        });
        let incomplete = json!({
            "amount_due": {"type": "integer"},
            "amount_paid": {"type": "integer"}
        });

        assert_eq!(
            detect(complete.as_object().unwrap_or_else(|| panic!("object"))).len(),
            1
        );
        assert!(detect(incomplete.as_object().unwrap_or_else(|| panic!("object"))).is_empty());
    }
}
