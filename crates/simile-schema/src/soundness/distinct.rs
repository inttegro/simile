//! Evaluates uniqueness across scalar values selected from an array.
//!
//! For example:
//!
//! ```yaml
//! x-simile-constraints:
//!   - distinct:
//!       source: /lines/*/id
//! ```
//!
//! OpenAPI `uniqueItems` compares complete array items; this evaluator compares the selected scalar
//! field instead. Compilation records a bounded pool of schema-valid witnesses. Proof reports every
//! repeated value after its first occurrence. Establishment replaces duplicates transactionally
//! with unused witnesses and never creates or removes array items.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Violation, ViolationKind, invalid, path, scalar};
use crate::{Result, SchemaCatalog, SchemaId, SchemaRef};

const MAX_WITNESSES: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Constraint {
    #[serde(rename = "s")]
    source: String,
    #[serde(rename = "w")]
    witnesses: Vec<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Pending {
    field: String,
    source: String,
}

pub(super) fn parse(definition: &Value, field: &str) -> Result<Pending> {
    let field = format!("{field}.distinct");
    let distinct = definition
        .as_object()
        .ok_or_else(|| invalid(&field, "distinct must be an object"))?;
    if distinct.len() != 1 || !distinct.contains_key("source") {
        return Err(invalid(&field, "distinct requires exactly `source`"));
    }
    let source = distinct
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(&format!("{field}.source"), "source must be a selector"))?;
    path::resolve_selector_syntax(source)
        .map_err(|reason| invalid(&format!("{field}.source"), &reason))?;
    Ok(Pending {
        field,
        source: source.to_owned(),
    })
}

pub(super) fn compile(
    catalog: &SchemaCatalog,
    root: SchemaId,
    pending: Pending,
) -> Result<Constraint> {
    let schema = resolve(catalog, root, &pending.source, &pending.field)?;
    let witnesses = witnesses(catalog, schema);
    if witnesses.len() < 2 {
        return Err(invalid(
            &pending.field,
            "distinct source schema must admit at least two constructible values",
        ));
    }
    Ok(Constraint {
        source: pending.source,
        witnesses,
    })
}

pub(super) fn is_well_formed(
    catalog: &SchemaCatalog,
    root: SchemaId,
    constraint: &Constraint,
) -> bool {
    let Some(schema) = resolve(catalog, root, &constraint.source, "distinct").ok() else {
        return false;
    };
    constraint.witnesses.len() >= 2
        && constraint
            .witnesses
            .iter()
            .enumerate()
            .all(|(index, witness)| {
                scalar::accepts(catalog, schema, witness)
                    && !constraint.witnesses[..index].contains(witness)
            })
}

pub(super) fn prove(constraint: &Constraint, value: &Value) -> Vec<Violation> {
    let mut seen = Vec::<Value>::new();
    path::select(value, &constraint.source)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|selected| {
            if seen.contains(selected.value) {
                Some(Violation {
                    kind: ViolationKind::Distinct,
                    path: selected.pointer,
                    message: "value must be distinct within the selected collection".to_owned(),
                })
            } else {
                seen.push(selected.value.clone());
                None
            }
        })
        .collect()
}

/// Replaces duplicate selected values with unused compiled witnesses transactionally.
pub(super) fn establish(constraint: &Constraint, value: &mut Value) -> bool {
    if prove(constraint, value).is_empty() {
        return false;
    }
    let selected = path::select(value, &constraint.source).unwrap_or_default();
    let entries = selected
        .into_iter()
        .map(|selected| (selected.pointer, selected.value.clone()))
        .collect::<Vec<_>>();
    let mut candidate = value.clone();
    let mut used = Vec::<Value>::new();
    for (pointer, current) in entries {
        if !used.contains(&current) {
            used.push(current);
            continue;
        }
        let Some(witness) = constraint
            .witnesses
            .iter()
            .find(|witness| !used.contains(witness))
            .cloned()
        else {
            return false;
        };
        if !path::apply(&mut candidate, &pointer, witness.clone()) {
            return false;
        }
        used.push(witness);
    }
    if candidate == *value || !prove(constraint, &candidate).is_empty() {
        return false;
    }
    *value = candidate;
    true
}

fn resolve(catalog: &SchemaCatalog, root: SchemaId, source: &str, field: &str) -> Result<SchemaId> {
    let selected = path::resolve_selector(catalog, root, source).ok_or_else(|| {
        invalid(
            field,
            &format!("selector `{source}` does not resolve to a declared value"),
        )
    })?;
    if !selected.multiple {
        return Err(invalid(
            field,
            "distinct source must contain an array wildcard `*`",
        ));
    }
    if scalar::kind(catalog, selected.schema).is_none() {
        return Err(invalid(field, "distinct source values must be scalar"));
    }
    Ok(selected.schema)
}

fn witnesses(catalog: &SchemaCatalog, schema: SchemaId) -> Vec<Value> {
    let mut witnesses = scalar::enum_values(catalog, schema).unwrap_or_default();
    witnesses.retain(|value| scalar::accepts(catalog, schema, value));
    let mut unique = Vec::new();
    for witness in witnesses {
        if !unique.contains(&witness) {
            unique.push(witness);
        }
    }
    while unique.len() < MAX_WITNESSES {
        let candidate = catalog
            .synthesize_representative_matching(SchemaRef(schema), |value| !unique.contains(value))
            .ok();
        let Some(candidate) = candidate else {
            break;
        };
        unique.push(candidate);
    }
    unique
}
