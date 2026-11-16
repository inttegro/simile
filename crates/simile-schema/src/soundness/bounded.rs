//! Evaluates a field against inclusive or exclusive bounds supplied by other fields.
//!
//! For example:
//!
//! ```yaml
//! x-simile-constraints:
//!   - bounded:
//!       path: /amount_paid
//!       minimum: { path: /amount_refunded }
//!       maximum: { path: /amount_due }
//! ```
//!
//! Every participant must use the same numeric or RFC 3339 date-time kind. Bound sources are
//! required and non-null so proof and synthesis always have an operand. Establishment changes only
//! the bounded field: an inclusive violation copies the boundary, while an exclusive violation
//! chooses the next representable integer, finite float, or date-time beyond that boundary. All
//! bounds are established transactionally, so contradictory dynamic bounds do not partially
//! mutate the payload.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use super::{Violation, ViolationKind, invalid, ordered, path, scalar};
use crate::{Result, SchemaCatalog, SchemaId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Constraint {
    #[serde(rename = "p")]
    path: String,
    #[serde(rename = "b")]
    bounds: Vec<Bound>,
    #[serde(rename = "k")]
    kind: ordered::Kind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Bound {
    #[serde(rename = "p")]
    path: String,
    #[serde(rename = "d")]
    direction: Direction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Direction {
    #[serde(rename = "m")]
    Minimum,
    #[serde(rename = "M")]
    Maximum,
    #[serde(rename = "x")]
    ExclusiveMinimum,
    #[serde(rename = "X")]
    ExclusiveMaximum,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Pending {
    field: String,
    path: String,
    bounds: Vec<Bound>,
}

pub(super) fn parse(definition: &Value, field: &str) -> Result<Pending> {
    let field = format!("{field}.bounded");
    let bounded = definition
        .as_object()
        .ok_or_else(|| invalid(&field, "bounded must be an object"))?;
    if bounded.keys().any(|key| {
        !matches!(
            key.as_str(),
            "path" | "minimum" | "maximum" | "exclusive-minimum" | "exclusive-maximum"
        )
    }) {
        return Err(invalid(
            &field,
            "bounded supports `path`, `minimum`, `maximum`, `exclusive-minimum`, and `exclusive-maximum`",
        ));
    }
    let target = pointer(bounded.get("path"), &format!("{field}.path"))?;
    let mut bounds = Vec::new();
    for (name, direction) in [
        ("minimum", Direction::Minimum),
        ("exclusive-minimum", Direction::ExclusiveMinimum),
        ("maximum", Direction::Maximum),
        ("exclusive-maximum", Direction::ExclusiveMaximum),
    ] {
        if let Some(value) = bounded.get(name) {
            bounds.push(Bound {
                path: operand(value, &format!("{field}.{name}"))?,
                direction,
            });
        }
    }
    if bounds.is_empty() {
        return Err(invalid(
            &field,
            "bounded requires at least one relative bound",
        ));
    }
    if bounds.iter().any(|bound| bound.path == target) {
        return Err(invalid(&field, "a bounded field cannot bound itself"));
    }
    Ok(Pending {
        field,
        path: target,
        bounds,
    })
}

pub(super) fn compile(
    catalog: &SchemaCatalog,
    root: SchemaId,
    pending: Pending,
) -> Result<Constraint> {
    let target = resolve(catalog, root, &pending.path, &pending.field, false)?;
    for bound in &pending.bounds {
        let kind = resolve(catalog, root, &bound.path, &pending.field, true)?;
        if kind != target {
            return Err(invalid(
                &pending.field,
                "bounded paths must use compatible value kinds",
            ));
        }
    }
    Ok(Constraint {
        path: pending.path,
        bounds: pending.bounds,
        kind: target,
    })
}

pub(super) fn is_well_formed(
    catalog: &SchemaCatalog,
    root: SchemaId,
    constraint: &Constraint,
) -> bool {
    !constraint.bounds.is_empty()
        && resolve(catalog, root, &constraint.path, "bounded", false).ok() == Some(constraint.kind)
        && constraint.bounds.iter().all(|bound| {
            bound.path != constraint.path
                && resolve(catalog, root, &bound.path, "bounded", true).ok()
                    == Some(constraint.kind)
        })
}

pub(super) fn prove(constraint: &Constraint, value: &Value) -> Vec<Violation> {
    let Some(target) = value
        .pointer(&constraint.path)
        .filter(|value| !value.is_null())
    else {
        return violation(constraint, "bounded value is missing or null");
    };
    constraint
        .bounds
        .iter()
        .filter_map(|bound| {
            let comparison = value
                .pointer(&bound.path)
                .filter(|value| !value.is_null())
                .and_then(|source| constraint.kind.compare(target, source));
            if comparison.is_some_and(|ordering| bound.direction.accepts(ordering)) {
                None
            } else {
                Some(Violation {
                    kind: ViolationKind::Bounded,
                    path: constraint.path.clone(),
                    message: format!(
                        "value must be {} `{}`",
                        bound.direction.description(),
                        bound.path
                    ),
                })
            }
        })
        .collect()
}

/// Repairs the bounded field against every source and commits only a completely sound candidate.
pub(super) fn establish(constraint: &Constraint, value: &mut Value) -> bool {
    if prove(constraint, value).is_empty() {
        return false;
    }
    let mut candidate = value.clone();
    for bound in &constraint.bounds {
        let Some(source) = candidate.pointer(&bound.path).cloned() else {
            return false;
        };
        if let Some(target) = candidate
            .pointer(&constraint.path)
            .filter(|target| !target.is_null())
        {
            let Some(ordering) = constraint.kind.compare(target, &source) else {
                return false;
            };
            if bound.direction.accepts(ordering) {
                continue;
            }
        }
        let Some(replacement) = bound.direction.replacement(constraint.kind, &source) else {
            return false;
        };
        if !path::apply(&mut candidate, &constraint.path, replacement) {
            return false;
        }
    }
    if candidate == *value || !prove(constraint, &candidate).is_empty() {
        return false;
    }
    *value = candidate;
    true
}

fn resolve(
    catalog: &SchemaCatalog,
    root: SchemaId,
    pointer: &str,
    field: &str,
    required: bool,
) -> Result<ordered::Kind> {
    let resolved = path::resolve_property(catalog, root, pointer).ok_or_else(|| {
        invalid(
            field,
            &format!("path `{pointer}` does not resolve to a property"),
        )
    })?;
    if required && !resolved.required {
        return Err(invalid(
            field,
            &format!("bound source `{pointer}` must be required by its schema"),
        ));
    }
    if required && scalar::accepts(catalog, resolved.schema, &Value::Null) {
        return Err(invalid(
            field,
            &format!("bound source `{pointer}` must not be nullable"),
        ));
    }
    ordered::kind_for_schema(catalog, resolved.schema).ok_or_else(|| {
        invalid(
            field,
            &format!(
                "path `{pointer}` must resolve to a number or a string with format `date-time`"
            ),
        )
    })
}

fn pointer(value: Option<&Value>, field: &str) -> Result<String> {
    let pointer = value
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(field, "value must be a JSON Pointer"))?;
    path::segments(pointer).map_err(|reason| invalid(field, &reason))?;
    Ok(pointer.to_owned())
}

fn operand(value: &Value, field: &str) -> Result<String> {
    let operand = value
        .as_object()
        .ok_or_else(|| invalid(field, "bound must be an object containing `path`"))?;
    if operand.len() != 1 || !operand.contains_key("path") {
        return Err(invalid(field, "bound requires exactly `path`"));
    }
    pointer(operand.get("path"), &format!("{field}.path"))
}

fn violation(constraint: &Constraint, message: &str) -> Vec<Violation> {
    vec![Violation {
        kind: ViolationKind::Bounded,
        path: constraint.path.clone(),
        message: message.to_owned(),
    }]
}

impl Direction {
    const fn accepts(self, ordering: Ordering) -> bool {
        match self {
            Self::Minimum => !matches!(ordering, Ordering::Less),
            Self::Maximum => !matches!(ordering, Ordering::Greater),
            Self::ExclusiveMinimum => matches!(ordering, Ordering::Greater),
            Self::ExclusiveMaximum => matches!(ordering, Ordering::Less),
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::Minimum => "greater than or equal to",
            Self::Maximum => "less than or equal to",
            Self::ExclusiveMinimum => "greater than",
            Self::ExclusiveMaximum => "less than",
        }
    }

    fn replacement(self, kind: ordered::Kind, source: &Value) -> Option<Value> {
        match self {
            Self::Minimum | Self::Maximum => Some(source.clone()),
            Self::ExclusiveMinimum => shift(kind, source, Ordering::Greater),
            Self::ExclusiveMaximum => shift(kind, source, Ordering::Less),
        }
    }
}

fn shift(kind: ordered::Kind, source: &Value, direction: Ordering) -> Option<Value> {
    match kind {
        ordered::Kind::Number => shift_number(source, direction),
        ordered::Kind::DateTime => {
            let value = OffsetDateTime::parse(source.as_str()?, &Rfc3339).ok()?;
            let shifted = match direction {
                Ordering::Greater => value.checked_add(Duration::nanoseconds(1))?,
                Ordering::Less => value.checked_sub(Duration::nanoseconds(1))?,
                Ordering::Equal => value,
            };
            shifted.format(&Rfc3339).ok().map(Value::String)
        }
    }
}

fn shift_number(source: &Value, direction: Ordering) -> Option<Value> {
    let number = source.as_number()?;
    if let Some(value) = number.as_i64() {
        return match direction {
            Ordering::Greater => value.checked_add(1),
            Ordering::Less => value.checked_sub(1),
            Ordering::Equal => Some(value),
        }
        .map(Number::from)
        .map(Value::Number);
    }
    if let Some(value) = number.as_u64() {
        return match direction {
            Ordering::Greater => value.checked_add(1).map(Number::from),
            Ordering::Less => value.checked_sub(1).map(Number::from),
            Ordering::Equal => Some(Number::from(value)),
        }
        .map(Value::Number);
    }
    let value = number.as_f64()?;
    let shifted = match direction {
        Ordering::Greater => next_up(value),
        Ordering::Less => next_down(value),
        Ordering::Equal => Some(value),
    }?;
    Number::from_f64(shifted).map(Value::Number)
}

fn next_up(value: f64) -> Option<f64> {
    if value.is_nan() || value == f64::INFINITY {
        return None;
    }
    if value == -0.0 {
        return Some(f64::from_bits(1));
    }
    let bits = value.to_bits();
    Some(f64::from_bits(if value >= 0.0 {
        bits + 1
    } else {
        bits - 1
    }))
}

fn next_down(value: f64) -> Option<f64> {
    if value.is_nan() || value == f64::NEG_INFINITY {
        return None;
    }
    if value == 0.0 {
        return Some(-f64::from_bits(1));
    }
    let bits = value.to_bits();
    Some(f64::from_bits(if value > 0.0 {
        bits - 1
    } else {
        bits + 1
    }))
}
