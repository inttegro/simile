//! Evaluates equality across fields and values selected from arrays.
//!
//! For example:
//!
//! ```yaml
//! x-simile-constraints:
//!   - equal:
//!       paths: [/currency, /lines/*/currency]
//! ```
//!
//! The `*` selector visits every current array item. Proof treats zero or one selected values as
//! vacuously equal and compares every later value with the first. During compilation, Simile finds
//! one value accepted by every selected schema. Establishment writes that common witness to exact
//! fields and to each existing wildcard match; it does not create new array items.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Violation, ViolationKind, invalid, path, scalar};
use crate::{Result, SchemaCatalog, SchemaId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Constraint {
    #[serde(rename = "p")]
    paths: Vec<ConstrainedPath>,
    #[serde(rename = "w")]
    witness: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct ConstrainedPath {
    #[serde(rename = "p")]
    selector: String,
    #[serde(rename = "m")]
    multiple: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Pending {
    field: String,
    paths: Vec<String>,
}

pub(super) fn parse(definition: &Value, field: &str) -> Result<Pending> {
    let field = format!("{field}.equal");
    let equal = definition
        .as_object()
        .ok_or_else(|| invalid(&field, "equal must be an object"))?;
    if equal.keys().any(|key| key != "paths") {
        return Err(invalid(&field, "equal currently supports only `paths`"));
    }
    let paths = equal
        .get("paths")
        .and_then(Value::as_array)
        .filter(|paths| paths.len() >= 2)
        .ok_or_else(|| {
            invalid(
                &format!("{field}.paths"),
                "paths must contain at least two selectors",
            )
        })?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let selector = value.as_str().ok_or_else(|| {
                invalid(
                    &format!("{field}.paths.{index}"),
                    "each path must be a selector string",
                )
            })?;
            path::resolve_selector_syntax(selector)
                .map_err(|reason| invalid(&format!("{field}.paths.{index}"), &reason))?;
            Ok(selector.to_owned())
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
    let resolved = resolve(catalog, root, &pending.paths, &pending.field)?;
    let schemas = resolved.iter().map(|item| item.0).collect::<Vec<_>>();
    let witness = scalar::common_witness(catalog, &schemas).ok_or_else(|| {
        invalid(
            &pending.field,
            "equal paths do not share a value allowed by every declared schema",
        )
    })?;
    Ok(Constraint {
        paths: pending
            .paths
            .into_iter()
            .zip(resolved)
            .map(|(selector, (_, multiple))| ConstrainedPath { selector, multiple })
            .collect(),
        witness,
    })
}

pub(super) fn is_well_formed(
    catalog: &SchemaCatalog,
    root: SchemaId,
    constraint: &Constraint,
) -> bool {
    if constraint.paths.len() < 2 {
        return false;
    }
    let paths = constraint
        .paths
        .iter()
        .map(|item| item.selector.clone())
        .collect::<Vec<_>>();
    if paths.iter().collect::<BTreeSet<_>>().len() != paths.len() {
        return false;
    }
    let Ok(resolved) = resolve(catalog, root, &paths, "equal") else {
        return false;
    };
    constraint
        .paths
        .iter()
        .zip(&resolved)
        .all(|(path, (_, multiple))| path.multiple == *multiple)
        && resolved
            .iter()
            .all(|(schema, _)| scalar::accepts(catalog, *schema, &constraint.witness))
}

pub(super) fn prove(constraint: &Constraint, value: &Value) -> Vec<Violation> {
    let selected = constraint
        .paths
        .iter()
        .flat_map(|item| path::select(value, &item.selector).unwrap_or_default())
        .collect::<Vec<_>>();
    let Some(expected) = selected.first().map(|selected| selected.value) else {
        return Vec::new();
    };
    selected
        .into_iter()
        .skip(1)
        .filter(|selected| selected.value != expected)
        .map(|selected| Violation {
            kind: ViolationKind::Equal,
            path: selected.pointer,
            message: "value must equal the other values in this relationship".to_owned(),
        })
        .collect()
}

/// Replaces every selected value with the compiled common witness when equality is violated.
pub(super) fn establish(constraint: &Constraint, value: &mut Value) -> bool {
    if prove(constraint, value).is_empty() {
        return false;
    }
    let before = value.clone();
    for item in &constraint.paths {
        let selected = path::select(value, &item.selector).unwrap_or_default();
        if item.multiple {
            let pointers = selected
                .into_iter()
                .map(|selected| selected.pointer)
                .collect::<Vec<_>>();
            for pointer in pointers {
                path::apply(value, &pointer, constraint.witness.clone());
            }
        } else {
            path::apply(value, &item.selector, constraint.witness.clone());
        }
    }
    before != *value && prove(constraint, value).is_empty()
}

fn resolve(
    catalog: &SchemaCatalog,
    root: SchemaId,
    paths: &[String],
    field: &str,
) -> Result<Vec<(SchemaId, bool)>> {
    let mut kind = None;
    paths
        .iter()
        .map(|selector| {
            let resolved = path::resolve_selector(catalog, root, selector).ok_or_else(|| {
                invalid(
                    field,
                    &format!("path `{selector}` does not resolve to a declared value"),
                )
            })?;
            let path_kind = scalar::kind(catalog, resolved.schema).ok_or_else(|| {
                invalid(
                    field,
                    &format!("path `{selector}` must resolve to a scalar value"),
                )
            })?;
            if kind.is_some_and(|kind| kind != path_kind) {
                return Err(invalid(field, "all equal paths must use compatible types"));
            }
            kind = Some(path_kind);
            Ok((resolved.schema, resolved.multiple))
        })
        .collect()
}
