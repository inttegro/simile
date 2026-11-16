//! Compiles, proves, and establishes relationships that make a payload sound as a whole.
//!
//! Shape validation remains in `validate`; this module owns every rule that relates multiple
//! values. Both validation and synthesis must delegate here so a generator cannot silently use a
//! weaker relationship model than the runtime validator.

mod aggregate;
mod comparison;
mod conditional;
mod equal;
mod equation;
mod lifecycle;
mod ordered;
mod path;
mod predicate;
mod scalar;
mod template;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{CompileError, Result, SchemaCatalog, SchemaId, SchemaNode};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum Constraint {
    #[serde(rename = "a")]
    Aggregate(aggregate::Constraint),
    #[serde(rename = "e")]
    Equal(equal::Constraint),
    #[serde(rename = "q")]
    Equation(equation::Constraint),
    #[serde(rename = "i")]
    Implies(conditional::Constraint),
    #[serde(rename = "l")]
    Lifecycle(lifecycle::Constraint),
    #[serde(rename = "o")]
    Ordered(ordered::Constraint),
    #[serde(rename = "t")]
    Template(template::Constraint),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PendingConstraint {
    Aggregate(aggregate::Pending),
    Equal(equal::Pending),
    Equation(equation::Pending),
    Implies(conditional::Pending),
    Lifecycle(lifecycle::Pending),
    Ordered(ordered::Pending),
    Template(template::Pending),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ViolationKind {
    Aggregate,
    Equal,
    Equation,
    Implies,
    Lifecycle,
    Ordered,
    Template,
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
                "aggregate" => aggregate::parse(definition, &constraint_field)
                    .map(PendingConstraint::Aggregate),
                "equal" => {
                    equal::parse(definition, &constraint_field).map(PendingConstraint::Equal)
                }
                "equation" => {
                    equation::parse(definition, &constraint_field).map(PendingConstraint::Equation)
                }
                "implies" => conditional::parse(definition, &constraint_field)
                    .map(PendingConstraint::Implies),
                "lifecycle" => lifecycle::parse(definition, &constraint_field)
                    .map(PendingConstraint::Lifecycle),
                "ordered" => {
                    ordered::parse(definition, &constraint_field).map(PendingConstraint::Ordered)
                }
                "template" => {
                    template::parse(definition, &constraint_field).map(PendingConstraint::Template)
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
        PendingConstraint::Aggregate(pending) => {
            aggregate::compile(catalog, root, pending).map(Constraint::Aggregate)
        }
        PendingConstraint::Equal(pending) => {
            equal::compile(catalog, root, pending).map(Constraint::Equal)
        }
        PendingConstraint::Equation(pending) => {
            equation::compile(catalog, root, pending).map(Constraint::Equation)
        }
        PendingConstraint::Implies(pending) => {
            conditional::compile(catalog, root, pending).map(Constraint::Implies)
        }
        PendingConstraint::Lifecycle(pending) => {
            lifecycle::compile(catalog, root, pending).map(Constraint::Lifecycle)
        }
        PendingConstraint::Ordered(pending) => {
            ordered::compile(catalog, root, pending).map(Constraint::Ordered)
        }
        PendingConstraint::Template(pending) => {
            template::compile(catalog, root, pending).map(Constraint::Template)
        }
    }
}

pub(crate) fn is_well_formed(
    catalog: &SchemaCatalog,
    root: SchemaId,
    constraints: &[Constraint],
) -> bool {
    constraints.iter().all(|constraint| match constraint {
        Constraint::Aggregate(constraint) => aggregate::is_well_formed(catalog, root, constraint),
        Constraint::Equal(constraint) => equal::is_well_formed(catalog, root, constraint),
        Constraint::Equation(constraint) => equation::is_well_formed(catalog, root, constraint),
        Constraint::Implies(constraint) => conditional::is_well_formed(catalog, root, constraint),
        Constraint::Lifecycle(constraint) => lifecycle::is_well_formed(catalog, root, constraint),
        Constraint::Ordered(constraint) => ordered::is_well_formed(catalog, root, constraint),
        Constraint::Template(constraint) => template::is_well_formed(catalog, root, constraint),
    })
}

pub(crate) fn prove(node: &SchemaNode, value: &Value) -> Vec<Violation> {
    node.constraints
        .iter()
        .flat_map(|constraint| match constraint {
            Constraint::Aggregate(constraint) => aggregate::prove(constraint, value),
            Constraint::Equal(constraint) => equal::prove(constraint, value),
            Constraint::Equation(constraint) => equation::prove(constraint, value),
            Constraint::Implies(constraint) => conditional::prove(constraint, value),
            Constraint::Lifecycle(constraint) => lifecycle::prove(constraint, value),
            Constraint::Ordered(constraint) => ordered::prove(constraint, value),
            Constraint::Template(constraint) => template::prove(constraint, value),
        })
        .collect()
}

pub(crate) fn establish(node: &SchemaNode, value: &mut Value) {
    let passes = node.constraints.len().saturating_mul(2).max(1);
    for _ in 0..passes {
        let mut changed = false;
        for constraint in &node.constraints {
            changed |= match constraint {
                Constraint::Aggregate(constraint) => aggregate::establish(constraint, value),
                Constraint::Equal(constraint) => equal::establish(constraint, value),
                Constraint::Equation(constraint) => equation::establish(constraint, value),
                Constraint::Implies(constraint) => conditional::establish(constraint, value),
                Constraint::Lifecycle(constraint) => lifecycle::establish(constraint, value),
                Constraint::Ordered(constraint) => ordered::establish(constraint, value),
                Constraint::Template(constraint) => template::establish(constraint, value),
            };
        }
        if !changed {
            break;
        }
    }
}

fn invalid(field: &str, reason: &str) -> CompileError {
    CompileError::InvalidConstraint {
        field: field.to_owned(),
        reason: reason.to_owned(),
    }
}
