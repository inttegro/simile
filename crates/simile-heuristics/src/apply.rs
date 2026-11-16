use serde_json::{Map, Number, Value};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::model::{Action, Application, Change, Rule};
use crate::schema::pointer;

pub(crate) fn rule(rule: &Rule, object: &mut Map<String, Value>) -> Option<Application> {
    let changes = match &rule.action {
        Action::Ordered { earlier, later } => ordered(object, earlier, later),
        Action::Count {
            collection,
            count: count_field,
        } => count(object, collection, count_field),
        Action::Difference {
            minuend,
            subtrahend,
            result,
        } => difference(object, minuend, subtrahend, result),
    };

    (!changes.is_empty()).then(|| Application {
        rule_id: rule.id.clone(),
        confidence: rule.confidence,
        changes,
    })
}

/// Repairs an inverted numeric or RFC 3339 range by copying the earlier value to the later field.
///
/// For example, `{ "created_at": "2026-01-02T00:00:00Z", "updated_at":
/// "2026-01-01T00:00:00Z" }` becomes a zero-width range whose `updated_at` equals `created_at`.
/// Missing, mismatched, or unparsable values are left untouched.
fn ordered(object: &mut Map<String, Value>, earlier: &str, later: &str) -> Vec<Change> {
    let Some(left) = object.get(earlier).cloned() else {
        return Vec::new();
    };
    let Some(right) = object.get(later).cloned() else {
        return Vec::new();
    };
    let in_order = match (&left, &right) {
        (Value::Number(left), Value::Number(right)) => left
            .as_f64()
            .zip(right.as_f64())
            .is_some_and(|(left, right)| left <= right),
        (Value::String(left), Value::String(right)) => parse_time(left)
            .zip(parse_time(right))
            .is_some_and(|(left, right)| left <= right),
        _ => true,
    };

    replace(object, later, right, left, !in_order)
}

/// Repairs an explicit singular count so it equals the adjacent array's length.
///
/// For example, `{ "items": ["a", "b"], "item_count": 9 }` becomes
/// `{ "items": ["a", "b"], "item_count": 2 }`. Missing arrays or count fields are left
/// untouched, and detection prevents this evaluator from running on pagination envelopes.
fn count(object: &mut Map<String, Value>, collection: &str, count: &str) -> Vec<Change> {
    let Some(length) = object
        .get(collection)
        .and_then(Value::as_array)
        .map(Vec::len)
    else {
        return Vec::new();
    };
    let Some(before) = object.get(count).cloned() else {
        return Vec::new();
    };
    let Ok(length) = u64::try_from(length) else {
        return Vec::new();
    };
    let after = Value::Number(Number::from(length));

    replace(
        object,
        count,
        before.clone(),
        after.clone(),
        before != after,
    )
}

/// Repairs the exact difference identity declared by an accounting heuristic.
///
/// For example, `amount_due = 10` and `amount_paid = 4` produce
/// `amount_remaining = 6`. This evaluator does not infer fees, credits, taxes, or balances; the
/// catalog only emits this action for the narrow three-field relationship at aggressive policy.
fn difference(
    object: &mut Map<String, Value>,
    minuend: &str,
    subtrahend: &str,
    result: &str,
) -> Vec<Change> {
    let Some(left) = object.get(minuend).and_then(Value::as_f64) else {
        return Vec::new();
    };
    let Some(right) = object.get(subtrahend).and_then(Value::as_f64) else {
        return Vec::new();
    };
    let Some(number) = Number::from_f64(left - right) else {
        return Vec::new();
    };
    let Some(before) = object.get(result).cloned() else {
        return Vec::new();
    };
    let after = Value::Number(number);

    replace(
        object,
        result,
        before.clone(),
        after.clone(),
        before != after,
    )
}

fn replace(
    object: &mut Map<String, Value>,
    field: &str,
    before: Value,
    after: Value,
    needed: bool,
) -> Vec<Change> {
    if !needed {
        return Vec::new();
    }
    object.insert(field.to_owned(), after.clone());
    vec![Change {
        path: pointer(field),
        before,
        after,
    }]
}

fn parse_time(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).ok()
}
