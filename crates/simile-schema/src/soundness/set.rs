//! Evaluates a set relationship between two scalar arrays.
//!
//! For example:
//!
//! ```yaml
//! x-simile-constraints:
//!   - set:
//!       left: /enabled_methods
//!       relation: subset-of
//!       right: /supported_methods
//! ```
//!
//! Arrays are compared as sets using JSON scalar equality. The left-hand array is the constructive
//! target and the required, non-null right-hand array is the reference. Establishment removes left
//! values for `subset-of` and `disjoint`, or appends missing reference values for `superset-of`.
//! It never mutates the reference array and commits only a result that satisfies the relationship.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Violation, ViolationKind, invalid, path, scalar};
use crate::{Result, SchemaCatalog, SchemaId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Constraint {
    #[serde(rename = "l")]
    left: String,
    #[serde(rename = "r")]
    right: String,
    #[serde(rename = "k")]
    relation: Relation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Relation {
    #[serde(rename = "s")]
    Subset,
    #[serde(rename = "S")]
    Superset,
    #[serde(rename = "d")]
    Disjoint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Pending {
    field: String,
    left: String,
    right: String,
    relation: Relation,
}

pub(super) fn parse(definition: &Value, field: &str) -> Result<Pending> {
    let field = format!("{field}.set");
    let set = definition
        .as_object()
        .ok_or_else(|| invalid(&field, "set must be an object"))?;
    if set.len() != 3
        || !set.contains_key("left")
        || !set.contains_key("relation")
        || !set.contains_key("right")
    {
        return Err(invalid(
            &field,
            "set requires exactly `left`, `relation`, and `right`",
        ));
    }
    let left = pointer(set.get("left"), &format!("{field}.left"))?;
    let right = pointer(set.get("right"), &format!("{field}.right"))?;
    if left == right {
        return Err(invalid(&field, "set arrays must use different paths"));
    }
    let relation = match set.get("relation").and_then(Value::as_str) {
        Some("subset-of") => Relation::Subset,
        Some("superset-of") => Relation::Superset,
        Some("disjoint") => Relation::Disjoint,
        Some(relation) => {
            return Err(invalid(
                &format!("{field}.relation"),
                &format!("unsupported set relation `{relation}`"),
            ));
        }
        None => {
            return Err(invalid(
                &format!("{field}.relation"),
                "relation must be a string",
            ));
        }
    };
    Ok(Pending {
        field,
        left,
        right,
        relation,
    })
}

pub(super) fn compile(
    catalog: &SchemaCatalog,
    root: SchemaId,
    pending: Pending,
) -> Result<Constraint> {
    check(catalog, root, &pending.left, &pending.right, &pending.field)?;
    Ok(Constraint {
        left: pending.left,
        right: pending.right,
        relation: pending.relation,
    })
}

pub(super) fn is_well_formed(
    catalog: &SchemaCatalog,
    root: SchemaId,
    constraint: &Constraint,
) -> bool {
    constraint.left != constraint.right
        && check(catalog, root, &constraint.left, &constraint.right, "set").is_ok()
}

pub(super) fn prove(constraint: &Constraint, value: &Value) -> Vec<Violation> {
    let Some(left) = value.pointer(&constraint.left).and_then(Value::as_array) else {
        return violation(constraint, "left-hand set is missing or is not an array");
    };
    let Some(right) = value.pointer(&constraint.right).and_then(Value::as_array) else {
        return violation(constraint, "right-hand set is missing or is not an array");
    };
    let sound = match constraint.relation {
        Relation::Subset => left.iter().all(|value| right.contains(value)),
        Relation::Superset => right.iter().all(|value| left.contains(value)),
        Relation::Disjoint => left.iter().all(|value| !right.contains(value)),
    };
    if sound {
        Vec::new()
    } else {
        violation(
            constraint,
            &format!(
                "left-hand set must be {} `{}`",
                constraint.relation.description(),
                constraint.right
            ),
        )
    }
}

/// Repairs only the left-hand array and reports whether a sound candidate was committed.
pub(super) fn establish(constraint: &Constraint, value: &mut Value) -> bool {
    if prove(constraint, value).is_empty() {
        return false;
    }
    let Some(right) = value.pointer(&constraint.right).and_then(Value::as_array) else {
        return false;
    };
    let right = right.clone();
    let left = value
        .pointer(&constraint.left)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let repaired = match constraint.relation {
        Relation::Subset => left
            .into_iter()
            .filter(|value| right.contains(value))
            .collect(),
        Relation::Superset => {
            let mut values = left;
            for value in right {
                if !values.contains(&value) {
                    values.push(value);
                }
            }
            values
        }
        Relation::Disjoint => left
            .into_iter()
            .filter(|value| !right.contains(value))
            .collect(),
    };
    let mut candidate = value.clone();
    if !path::apply(&mut candidate, &constraint.left, Value::Array(repaired))
        || candidate == *value
        || !prove(constraint, &candidate).is_empty()
    {
        return false;
    }
    *value = candidate;
    true
}

fn check(
    catalog: &SchemaCatalog,
    root: SchemaId,
    left: &str,
    right: &str,
    field: &str,
) -> Result<()> {
    let left = array(catalog, root, left, field, false)?;
    let right = array(catalog, root, right, field, true)?;
    let left_kind = scalar::kind(catalog, left);
    let right_kind = scalar::kind(catalog, right);
    if left_kind.is_none() || left_kind != right_kind {
        return Err(invalid(
            field,
            "set arrays must contain compatible scalar values",
        ));
    }
    Ok(())
}

fn array(
    catalog: &SchemaCatalog,
    root: SchemaId,
    pointer: &str,
    field: &str,
    required: bool,
) -> Result<SchemaId> {
    let resolved = path::resolve_property(catalog, root, pointer).ok_or_else(|| {
        invalid(
            field,
            &format!("path `{pointer}` does not resolve to a property"),
        )
    })?;
    if required && !resolved.required {
        return Err(invalid(
            field,
            "right-hand set must be required by its schema",
        ));
    }
    if required && scalar::accepts(catalog, resolved.schema, &Value::Null) {
        return Err(invalid(field, "right-hand set must not be nullable"));
    }
    path::item_schema(catalog, resolved.schema)
        .ok_or_else(|| invalid(field, &format!("path `{pointer}` must resolve to an array")))
}

fn pointer(value: Option<&Value>, field: &str) -> Result<String> {
    let pointer = value
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(field, "value must be a JSON Pointer"))?;
    path::segments(pointer).map_err(|reason| invalid(field, &reason))?;
    Ok(pointer.to_owned())
}

fn violation(constraint: &Constraint, message: &str) -> Vec<Violation> {
    vec![Violation {
        kind: ViolationKind::Set,
        path: constraint.left.clone(),
        message: message.to_owned(),
    }]
}

impl Relation {
    const fn description(self) -> &'static str {
        match self {
            Self::Subset => "a subset of",
            Self::Superset => "a superset of",
            Self::Disjoint => "disjoint from",
        }
    }
}
