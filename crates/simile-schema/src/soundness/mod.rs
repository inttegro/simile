//! Compiles, proves, and establishes relationships that make a payload sound as a whole.
//!
//! Shape validation remains in `validate`; this module owns every rule that relates multiple
//! values. Both validation and synthesis must delegate here so a generator cannot silently use a
//! weaker relationship model than the runtime validator.

mod ordered;
mod path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{CompileError, Result, SchemaCatalog, SchemaId, SchemaNode};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum Constraint {
    #[serde(rename = "o")]
    Ordered(ordered::Constraint),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PendingConstraint {
    Ordered(ordered::Pending),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ViolationKind {
    Ordered,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Violation {
    pub kind: ViolationKind,
    pub path: String,
    pub message: String,
}

pub(crate) fn parse(schema: &Map<String, Value>, field: &str) -> Result<Vec<PendingConstraint>> {
    let Some(value) = schema.get("x-simile-constraints") else {
        return Ok(Vec::new());
    };
    let constraints = value
        .as_array()
        .ok_or_else(|| invalid(field, "x-simile-constraints must be an array"))?;
    constraints
        .iter()
        .enumerate()
        .map(|(index, constraint)| {
            let constraint_field = format!("{field}.x-simile-constraints.{index}");
            let object = constraint
                .as_object()
                .ok_or_else(|| invalid(&constraint_field, "each constraint must be an object"))?;
            if object.len() != 1 {
                return Err(invalid(
                    &constraint_field,
                    "exactly one supported constraint kind is required",
                ));
            }
            let (kind, definition) = object
                .iter()
                .next()
                .ok_or_else(|| invalid(&constraint_field, "constraint kind is required"))?;
            match kind.as_str() {
                "ordered" => {
                    ordered::parse(definition, &constraint_field).map(PendingConstraint::Ordered)
                }
                _ => Err(invalid(
                    &constraint_field,
                    &format!("unsupported constraint kind `{kind}`"),
                )),
            }
        })
        .collect()
}

pub(crate) fn compile(
    catalog: &SchemaCatalog,
    root: SchemaId,
    constraint: PendingConstraint,
) -> Result<Constraint> {
    match constraint {
        PendingConstraint::Ordered(pending) => {
            ordered::compile(catalog, root, pending).map(Constraint::Ordered)
        }
    }
}

pub(crate) fn is_well_formed(
    catalog: &SchemaCatalog,
    root: SchemaId,
    constraints: &[Constraint],
) -> bool {
    constraints.iter().all(|constraint| match constraint {
        Constraint::Ordered(constraint) => ordered::is_well_formed(catalog, root, constraint),
    })
}

pub(crate) fn prove(node: &SchemaNode, value: &Value) -> Vec<Violation> {
    node.constraints
        .iter()
        .flat_map(|constraint| match constraint {
            Constraint::Ordered(constraint) => ordered::prove(constraint, value),
        })
        .collect()
}

pub(crate) fn establish(node: &SchemaNode, value: &mut Value) {
    for constraint in &node.constraints {
        match constraint {
            Constraint::Ordered(constraint) => ordered::establish(constraint, value),
        }
    }
}

fn invalid(field: &str, reason: &str) -> CompileError {
    CompileError::InvalidConstraint {
        field: field.to_owned(),
        reason: reason.to_owned(),
    }
}
