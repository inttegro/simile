//! Evaluates ordered field comparisons used as implication antecedents.
//!
//! For example:
//!
//! ```yaml
//! when:
//!   compare:
//!     left: /amount_paid
//!     operator: greater-than
//!     right: /amount_due
//! ```
//!
//! Both operands must compile to the same numeric or RFC 3339 date-time kind. A missing, null, or
//! unparseable operand makes the condition false, which leaves its implication vacuously sound.
//! Comparisons are read-only: consequence predicates, not this evaluator, perform repairs.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::{invalid, path};
use crate::{JsonType, Result, SchemaCatalog, SchemaId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct Condition {
    #[serde(rename = "l")]
    left: String,
    #[serde(rename = "o")]
    operator: Operator,
    #[serde(rename = "r")]
    right: String,
    #[serde(rename = "k")]
    kind: Kind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Pending {
    field: String,
    left: String,
    operator: Operator,
    right: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Operator {
    #[serde(rename = "g")]
    GreaterThan,
    #[serde(rename = "G")]
    GreaterThanOrEqual,
    #[serde(rename = "l")]
    LessThan,
    #[serde(rename = "L")]
    LessThanOrEqual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Kind {
    #[serde(rename = "n")]
    Number,
    #[serde(rename = "t")]
    DateTime,
}

pub(super) fn parse(value: &Value, field: &str) -> Result<Pending> {
    let comparison = value
        .as_object()
        .ok_or_else(|| invalid(field, "compare must be an object"))?;
    if comparison.len() != 3
        || !comparison.contains_key("left")
        || !comparison.contains_key("operator")
        || !comparison.contains_key("right")
    {
        return Err(invalid(
            field,
            "compare requires exactly `left`, `operator`, and `right`",
        ));
    }
    let left = operand(comparison.get("left"), &format!("{field}.left"))?;
    let right = operand(comparison.get("right"), &format!("{field}.right"))?;
    if left == right {
        return Err(invalid(field, "compare operands must use different paths"));
    }
    let operator = match comparison.get("operator").and_then(Value::as_str) {
        Some("greater-than") => Operator::GreaterThan,
        Some("greater-than-or-equal") => Operator::GreaterThanOrEqual,
        Some("less-than") => Operator::LessThan,
        Some("less-than-or-equal") => Operator::LessThanOrEqual,
        Some(operator) => {
            return Err(invalid(
                &format!("{field}.operator"),
                &format!("unsupported comparison operator `{operator}`"),
            ));
        }
        None => {
            return Err(invalid(
                &format!("{field}.operator"),
                "operator must be a string",
            ));
        }
    };
    Ok(Pending {
        field: field.to_owned(),
        left,
        operator,
        right,
    })
}

pub(super) fn compile(
    catalog: &SchemaCatalog,
    root: SchemaId,
    pending: Pending,
) -> Result<Condition> {
    let left = resolve(catalog, root, &pending.left, &pending.field)?;
    let right = resolve(catalog, root, &pending.right, &pending.field)?;
    if left != right {
        return Err(invalid(
            &pending.field,
            "compare operands must use compatible value types",
        ));
    }
    Ok(Condition {
        left: pending.left,
        operator: pending.operator,
        right: pending.right,
        kind: left,
    })
}

pub(super) fn is_well_formed(
    catalog: &SchemaCatalog,
    root: SchemaId,
    condition: &Condition,
) -> bool {
    condition.left != condition.right
        && resolve(catalog, root, &condition.left, "compare").ok() == Some(condition.kind)
        && resolve(catalog, root, &condition.right, "compare").ok() == Some(condition.kind)
}

impl Condition {
    /// Returns true only when both operands are present, comparable, and satisfy the operator.
    pub(super) fn matches(&self, value: &Value) -> bool {
        let Some(ordering) = value
            .pointer(&self.left)
            .zip(value.pointer(&self.right))
            .and_then(|(left, right)| self.kind.compare(left, right))
        else {
            return false;
        };
        self.operator.matches(ordering)
    }

    pub(super) fn description(&self) -> String {
        format!(
            "`{}` is {} `{}`",
            self.left,
            self.operator.description(),
            self.right
        )
    }
}

impl Operator {
    const fn matches(self, ordering: Ordering) -> bool {
        match self {
            Self::GreaterThan => matches!(ordering, Ordering::Greater),
            Self::GreaterThanOrEqual => !matches!(ordering, Ordering::Less),
            Self::LessThan => matches!(ordering, Ordering::Less),
            Self::LessThanOrEqual => !matches!(ordering, Ordering::Greater),
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::GreaterThan => "greater than",
            Self::GreaterThanOrEqual => "greater than or equal to",
            Self::LessThan => "less than",
            Self::LessThanOrEqual => "less than or equal to",
        }
    }
}

impl Kind {
    fn compare(self, left: &Value, right: &Value) -> Option<Ordering> {
        match self {
            Self::Number => left.as_f64()?.partial_cmp(&right.as_f64()?),
            Self::DateTime => {
                let left = OffsetDateTime::parse(left.as_str()?, &Rfc3339).ok()?;
                let right = OffsetDateTime::parse(right.as_str()?, &Rfc3339).ok()?;
                Some(left.cmp(&right))
            }
        }
    }
}

fn operand(value: Option<&Value>, field: &str) -> Result<String> {
    let operand = value
        .and_then(Value::as_object)
        .ok_or_else(|| invalid(field, "operand must be an object containing `path`"))?;
    if operand.len() != 1 || !operand.contains_key("path") {
        return Err(invalid(field, "operand requires exactly `path`"));
    }
    let pointer = operand
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(&format!("{field}.path"), "path must be a JSON Pointer"))?;
    path::segments(pointer).map_err(|reason| invalid(&format!("{field}.path"), &reason))?;
    Ok(pointer.to_owned())
}

fn resolve(catalog: &SchemaCatalog, root: SchemaId, pointer: &str, field: &str) -> Result<Kind> {
    let schema = path::resolve_property(catalog, root, pointer)
        .map(|resolved| resolved.schema)
        .ok_or_else(|| {
            invalid(
                field,
                &format!("path `{pointer}` does not resolve to a property"),
            )
        })?;
    kind_for_schema(catalog, schema).ok_or_else(|| {
        invalid(
            field,
            &format!(
                "path `{pointer}` must resolve to a number or a string with format `date-time`"
            ),
        )
    })
}

fn kind_for_schema(catalog: &SchemaCatalog, id: SchemaId) -> Option<Kind> {
    kind_for_schema_inner(catalog, id, &mut BTreeSet::new())
}

fn kind_for_schema_inner(
    catalog: &SchemaCatalog,
    id: SchemaId,
    visited: &mut BTreeSet<SchemaId>,
) -> Option<Kind> {
    if !visited.insert(id) {
        return None;
    }
    let node = catalog.nodes.get(id as usize)?;
    let non_null = node
        .types
        .iter()
        .filter(|kind| **kind != JsonType::Null)
        .copied()
        .collect::<BTreeSet<_>>();
    let direct = if !non_null.is_empty()
        && non_null
            .iter()
            .all(|kind| matches!(kind, JsonType::Integer | JsonType::Number))
    {
        Some(Kind::Number)
    } else if non_null == BTreeSet::from([JsonType::String])
        && node.format.as_deref() == Some("date-time")
    {
        Some(Kind::DateTime)
    } else {
        None
    };
    direct.or_else(|| {
        node.all_of.iter().find_map(|child| {
            let mut branch = visited.clone();
            kind_for_schema_inner(catalog, *child, &mut branch)
        })
    })
}
