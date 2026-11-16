use std::cmp::Ordering;
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::{Violation, ViolationKind, invalid, path};
use crate::{JsonType, Result, SchemaCatalog, SchemaId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Constraint {
    #[serde(rename = "p")]
    paths: Vec<String>,
    #[serde(rename = "k")]
    kind: Kind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Kind {
    #[serde(rename = "n")]
    Number,
    #[serde(rename = "t")]
    DateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Pending {
    field: String,
    paths: Vec<String>,
}

pub(super) fn parse(definition: &Value, field: &str) -> Result<Pending> {
    let field = format!("{field}.ordered");
    let ordered = definition
        .as_object()
        .ok_or_else(|| invalid(&field, "ordered must be an object"))?;
    if ordered.keys().any(|key| key != "paths") {
        return Err(invalid(&field, "ordered currently supports only `paths`"));
    }
    let paths = ordered
        .get("paths")
        .and_then(Value::as_array)
        .filter(|paths| paths.len() >= 2)
        .ok_or_else(|| {
            invalid(
                &format!("{field}.paths"),
                "paths must be an array containing at least two JSON Pointers",
            )
        })?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let pointer = value.as_str().ok_or_else(|| {
                invalid(
                    &format!("{field}.paths.{index}"),
                    "each path must be a JSON Pointer string",
                )
            })?;
            path::segments(pointer)
                .map_err(|reason| invalid(&format!("{field}.paths.{index}"), &reason))?;
            Ok(pointer.to_owned())
        })
        .collect::<Result<Vec<_>>>()?;
    if paths.iter().collect::<BTreeSet<_>>().len() != paths.len() {
        return Err(invalid(
            &format!("{field}.paths"),
            "paths must not contain duplicates",
        ));
    }
    Ok(Pending { field, paths })
}

pub(super) fn compile(
    catalog: &SchemaCatalog,
    root: SchemaId,
    pending: Pending,
) -> Result<Constraint> {
    let mut kind = None;
    for pointer in &pending.paths {
        let schema = path::resolve_property(catalog, root, pointer).ok_or_else(|| {
            invalid(
                &pending.field,
                &format!("path `{pointer}` does not resolve to a property"),
            )
        })?;
        let path_kind = kind_for_schema(catalog, schema).ok_or_else(|| {
            invalid(
                &pending.field,
                &format!(
                    "path `{pointer}` must resolve to a number or a string with format `date-time`"
                ),
            )
        })?;
        if kind.is_some_and(|kind| kind != path_kind) {
            return Err(invalid(
                &pending.field,
                "all ordered paths must use compatible value types",
            ));
        }
        kind = Some(path_kind);
    }
    Ok(Constraint {
        paths: pending.paths,
        kind: kind.ok_or_else(|| invalid(&pending.field, "ordered requires at least two paths"))?,
    })
}

pub(super) fn is_well_formed(
    catalog: &SchemaCatalog,
    root: SchemaId,
    constraint: &Constraint,
) -> bool {
    constraint.paths.len() >= 2
        && constraint.paths.iter().collect::<BTreeSet<_>>().len() == constraint.paths.len()
        && constraint.paths.iter().all(|pointer| {
            path::resolve_property(catalog, root, pointer)
                .and_then(|id| kind_for_schema(catalog, id))
                == Some(constraint.kind)
        })
}

pub(super) fn prove(constraint: &Constraint, value: &Value) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut previous: Option<(&str, &Value)> = None;
    for pointer in &constraint.paths {
        let Some(current) = value.pointer(pointer) else {
            continue;
        };
        if current.is_null() {
            continue;
        }
        if let Some((previous_path, previous_value)) = previous {
            match constraint.kind.compare(previous_value, current) {
                Some(Ordering::Greater) => violations.push(Violation {
                    kind: ViolationKind::Ordered,
                    path: pointer.clone(),
                    message: format!("value must be greater than or equal to `{previous_path}`"),
                }),
                Some(Ordering::Less | Ordering::Equal) => {}
                None => violations.push(Violation {
                    kind: ViolationKind::Ordered,
                    path: pointer.clone(),
                    message: "value cannot be compared by this ordered constraint".to_owned(),
                }),
            }
        }
        previous = Some((pointer, current));
    }
    violations
}

pub(super) fn establish(constraint: &Constraint, value: &mut Value) {
    let mut values = constraint
        .paths
        .iter()
        .filter_map(|pointer| {
            value
                .pointer(pointer)
                .filter(|value| !value.is_null())
                .cloned()
                .map(|value| (pointer, value))
        })
        .collect::<Vec<_>>();
    if values
        .iter()
        .any(|(_, value)| constraint.kind.compare(value, value) != Some(Ordering::Equal))
    {
        return;
    }
    let targets = values
        .iter()
        .map(|(pointer, _)| pointer.as_str())
        .collect::<Vec<_>>();
    values.sort_by(|left, right| {
        constraint
            .kind
            .compare(&left.1, &right.1)
            .unwrap_or(Ordering::Equal)
    });
    for (pointer, (_, ordered_value)) in targets.into_iter().zip(values) {
        if let Some(target) = value.pointer_mut(pointer) {
            *target = ordered_value;
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
