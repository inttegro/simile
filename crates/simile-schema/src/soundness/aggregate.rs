//! Evaluates a numeric aggregate over values selected from an array.
//!
//! For example:
//!
//! ```yaml
//! x-simile-constraints:
//!   - aggregate:
//!       target: /subtotal
//!       operation: sum
//!       source: /lines/*/amount
//!       when: { path: /status, equals: open }
//! ```
//!
//! The optional `when` predicate guards the whole relationship: a false guard is vacuously sound
//! and is not repaired. The source must contain an array wildcard. `sum`, `min`, and `max` select
//! non-null numbers; `count` may select any value. `sum` and `count` fold an empty selection to
//! zero, while `min` and `max` require at least one selected value. Proof compares the aggregate
//! with the target; establishment writes the computed number only when the guard matches.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Violation, ViolationKind, invalid, path, predicate, scalar};
use crate::{Result, SchemaCatalog, SchemaId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Constraint {
    #[serde(rename = "t")]
    target: String,
    #[serde(rename = "o")]
    operation: Operation,
    #[serde(rename = "s")]
    source: String,
    #[serde(rename = "w", skip_serializing_if = "Option::is_none")]
    when: Option<predicate::Predicate>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Operation {
    #[serde(rename = "s")]
    Sum,
    #[serde(rename = "c")]
    Count,
    #[serde(rename = "n")]
    Min,
    #[serde(rename = "x")]
    Max,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Pending {
    field: String,
    target: String,
    operation: Operation,
    source: String,
    when: Option<predicate::Pending>,
}

pub(super) fn parse(definition: &Value, field: &str) -> Result<Pending> {
    let field = format!("{field}.aggregate");
    let aggregate = definition
        .as_object()
        .ok_or_else(|| invalid(&field, "aggregate must be an object"))?;
    if aggregate
        .keys()
        .any(|key| !matches!(key.as_str(), "target" | "operation" | "source" | "when"))
        || !aggregate.contains_key("target")
        || !aggregate.contains_key("operation")
        || !aggregate.contains_key("source")
    {
        return Err(invalid(
            &field,
            "aggregate requires `target`, `operation`, and `source`, with optional `when`",
        ));
    }
    let target = pointer(aggregate.get("target"), &format!("{field}.target"))?;
    let source = aggregate
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(&format!("{field}.source"), "source must be a selector"))?;
    path::resolve_selector_syntax(source)
        .map_err(|reason| invalid(&format!("{field}.source"), &reason))?;
    let operation = match aggregate.get("operation").and_then(Value::as_str) {
        Some("sum") => Operation::Sum,
        Some("count") => Operation::Count,
        Some("min") => Operation::Min,
        Some("max") => Operation::Max,
        Some(operation) => {
            return Err(invalid(
                &format!("{field}.operation"),
                &format!("unsupported aggregate operation `{operation}`"),
            ));
        }
        None => {
            return Err(invalid(
                &format!("{field}.operation"),
                "operation must be a string",
            ));
        }
    };
    let when = aggregate
        .get("when")
        .map(|value| predicate::parse(value, &format!("{field}.when")))
        .transpose()?;
    Ok(Pending {
        field,
        target,
        operation,
        source: source.to_owned(),
        when,
    })
}

pub(super) fn compile(
    catalog: &SchemaCatalog,
    root: SchemaId,
    pending: Pending,
) -> Result<Constraint> {
    check_paths(
        catalog,
        root,
        &pending.target,
        &pending.source,
        pending.operation,
        &pending.field,
    )?;
    let when = pending
        .when
        .map(|pending| predicate::compile(catalog, root, pending))
        .transpose()?;
    Ok(Constraint {
        target: pending.target,
        operation: pending.operation,
        source: pending.source,
        when,
    })
}

pub(super) fn is_well_formed(
    catalog: &SchemaCatalog,
    root: SchemaId,
    constraint: &Constraint,
) -> bool {
    check_paths(
        catalog,
        root,
        &constraint.target,
        &constraint.source,
        constraint.operation,
        "aggregate",
    )
    .is_ok()
        && constraint
            .when
            .as_ref()
            .is_none_or(|when| predicate::is_well_formed(catalog, root, when))
}

pub(super) fn prove(constraint: &Constraint, value: &Value) -> Vec<Violation> {
    if constraint
        .when
        .as_ref()
        .is_some_and(|when| !when.matches(value))
    {
        return Vec::new();
    }
    let expected = evaluate(constraint, value);
    let actual = value
        .pointer(&constraint.target)
        .and_then(scalar::Numeric::from_value);
    if expected.is_some() && expected == actual {
        Vec::new()
    } else {
        vec![Violation {
            kind: ViolationKind::Aggregate,
            path: constraint.target.clone(),
            message: format!(
                "value must equal the {} of `{}`",
                constraint.operation.name(),
                constraint.source
            ),
        }]
    }
}

/// Computes the guarded aggregate, writes it to the target, and reports whether it changed.
pub(super) fn establish(constraint: &Constraint, value: &mut Value) -> bool {
    if constraint
        .when
        .as_ref()
        .is_some_and(|when| !when.matches(value))
    {
        return false;
    }
    let Some(expected) = evaluate(constraint, value).and_then(scalar::Numeric::into_value) else {
        return false;
    };
    if value.pointer(&constraint.target) == Some(&expected) {
        return false;
    }
    path::apply(value, &constraint.target, expected) && prove(constraint, value).is_empty()
}

fn evaluate(constraint: &Constraint, value: &Value) -> Option<scalar::Numeric> {
    let selected = path::select(value, &constraint.source)?;
    match constraint.operation {
        Operation::Sum => selected
            .into_iter()
            .try_fold(scalar::Numeric::Integer(0), |sum, selected| {
                sum.add(scalar::Numeric::from_value(selected.value)?)
            }),
        Operation::Count => i128::try_from(selected.len())
            .ok()
            .map(scalar::Numeric::Integer),
        Operation::Min => {
            let mut values = selected
                .into_iter()
                .map(|selected| scalar::Numeric::from_value(selected.value));
            let first = values.next()??;
            values.try_fold(first, |minimum, value| minimum.minimum(value?))
        }
        Operation::Max => {
            let mut values = selected
                .into_iter()
                .map(|selected| scalar::Numeric::from_value(selected.value));
            let first = values.next()??;
            values.try_fold(first, |maximum, value| maximum.maximum(value?))
        }
    }
}

fn check_paths(
    catalog: &SchemaCatalog,
    root: SchemaId,
    target: &str,
    source: &str,
    operation: Operation,
    field: &str,
) -> Result<()> {
    let target = path::resolve_property(catalog, root, target)
        .ok_or_else(|| invalid(field, "aggregate target does not resolve to a property"))?;
    if scalar::kind(catalog, target.schema) != Some(scalar::Kind::Number) {
        return Err(invalid(field, "aggregate target must be numeric"));
    }
    let source = path::resolve_selector(catalog, root, source).ok_or_else(|| {
        invalid(
            field,
            "aggregate source does not resolve to declared array items",
        )
    })?;
    if !source.multiple {
        return Err(invalid(
            field,
            "aggregate source must contain an array wildcard `*`",
        ));
    }
    if operation != Operation::Count {
        if scalar::kind(catalog, source.schema) != Some(scalar::Kind::Number) {
            return Err(invalid(
                field,
                "numeric aggregate source values must be numeric",
            ));
        }
        if scalar::accepts(catalog, source.schema, &Value::Null) {
            return Err(invalid(
                field,
                "numeric aggregate source values must not be nullable",
            ));
        }
    }
    Ok(())
}

fn pointer(value: Option<&Value>, field: &str) -> Result<String> {
    let pointer = value
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(field, "value must be a JSON Pointer"))?;
    path::segments(pointer).map_err(|reason| invalid(field, &reason))?;
    Ok(pointer.to_owned())
}

impl Operation {
    const fn name(self) -> &'static str {
        match self {
            Self::Sum => "sum",
            Self::Count => "count",
            Self::Min => "minimum",
            Self::Max => "maximum",
        }
    }
}
