//! Evaluates a numeric target derived from other numeric fields.
//!
//! For example:
//!
//! ```yaml
//! x-simile-constraints:
//!   - equation:
//!       target: /amount_remaining
//!       value:
//!         subtract:
//!           - { path: /amount_due }
//!           - { path: /amount_paid }
//! ```
//!
//! `add` accepts two or more expressions and `subtract` accepts exactly two. Source paths must be
//! required, non-null numeric fields, and the target cannot read itself. Proof evaluates the
//! expression and compares it with the target. Establishment calculates the same expression and
//! replaces the target while preserving exact integer arithmetic whenever every operand is an
//! integer.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Violation, ViolationKind, invalid, path, scalar};
use crate::{Result, SchemaCatalog, SchemaId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Constraint {
    #[serde(rename = "t")]
    target: String,
    #[serde(rename = "v")]
    value: Expression,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Expression {
    #[serde(rename = "p")]
    Path(String),
    #[serde(rename = "a")]
    Add(Vec<Expression>),
    #[serde(rename = "s")]
    Subtract(Box<[Expression; 2]>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Pending {
    field: String,
    target: String,
    value: Expression,
}

pub(super) fn parse(definition: &Value, field: &str) -> Result<Pending> {
    let field = format!("{field}.equation");
    let equation = definition
        .as_object()
        .ok_or_else(|| invalid(&field, "equation must be an object"))?;
    if equation.len() != 2 || !equation.contains_key("target") || !equation.contains_key("value") {
        return Err(invalid(
            &field,
            "equation requires exactly `target` and `value`",
        ));
    }
    let target = equation
        .get("target")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(&format!("{field}.target"), "target must be a JSON Pointer"))?;
    path::segments(target).map_err(|reason| invalid(&format!("{field}.target"), &reason))?;
    let value = parse_expression(
        equation
            .get("value")
            .ok_or_else(|| invalid(&format!("{field}.value"), "value is required"))?,
        &format!("{field}.value"),
    )?;
    if value.paths().any(|path| path == target) {
        return Err(invalid(
            &format!("{field}.value"),
            "an equation cannot read from its own target",
        ));
    }
    Ok(Pending {
        field,
        target: target.to_owned(),
        value,
    })
}

pub(super) fn compile(
    catalog: &SchemaCatalog,
    root: SchemaId,
    pending: Pending,
) -> Result<Constraint> {
    require_number(catalog, root, &pending.target, &pending.field, false)?;
    for source in pending.value.paths() {
        require_number(catalog, root, source, &pending.field, true)?;
    }
    Ok(Constraint {
        target: pending.target,
        value: pending.value,
    })
}

pub(super) fn is_well_formed(
    catalog: &SchemaCatalog,
    root: SchemaId,
    constraint: &Constraint,
) -> bool {
    require_number(catalog, root, &constraint.target, "equation", false).is_ok()
        && constraint.value.is_well_formed()
        && constraint
            .value
            .paths()
            .all(|source| require_number(catalog, root, source, "equation", true).is_ok())
        && constraint
            .value
            .paths()
            .all(|source| source != constraint.target)
}

pub(super) fn prove(constraint: &Constraint, value: &Value) -> Vec<Violation> {
    let expected = constraint.value.evaluate(value);
    let actual = value
        .pointer(&constraint.target)
        .and_then(scalar::Numeric::from_value);
    if expected.is_some() && expected == actual {
        Vec::new()
    } else {
        vec![Violation {
            kind: ViolationKind::Equation,
            path: constraint.target.clone(),
            message: "value must equal the declared numeric equation".to_owned(),
        }]
    }
}

/// Writes the evaluated expression to the target and returns whether the payload changed.
pub(super) fn establish(constraint: &Constraint, value: &mut Value) -> bool {
    let Some(expected) = constraint
        .value
        .evaluate(value)
        .and_then(scalar::Numeric::into_value)
    else {
        return false;
    };
    if value.pointer(&constraint.target) == Some(&expected) {
        return false;
    }
    path::apply(value, &constraint.target, expected) && prove(constraint, value).is_empty()
}

fn parse_expression(value: &Value, field: &str) -> Result<Expression> {
    let expression = value
        .as_object()
        .ok_or_else(|| invalid(field, "numeric expression must be an object"))?;
    if expression.len() != 1 {
        return Err(invalid(
            field,
            "numeric expression requires exactly one of `path`, `add`, or `subtract`",
        ));
    }
    let (operator, value) = expression
        .iter()
        .next()
        .ok_or_else(|| invalid(field, "numeric expression is required"))?;
    match operator.as_str() {
        "path" => {
            let pointer = value
                .as_str()
                .ok_or_else(|| invalid(&format!("{field}.path"), "path must be a JSON Pointer"))?;
            path::segments(pointer).map_err(|reason| invalid(&format!("{field}.path"), &reason))?;
            Ok(Expression::Path(pointer.to_owned()))
        }
        "add" => {
            let operands = parse_operands(value, &format!("{field}.add"), 2, None)?;
            Ok(Expression::Add(operands))
        }
        "subtract" => {
            let operands = parse_operands(value, &format!("{field}.subtract"), 2, Some(2))?;
            let operands = <[Expression; 2]>::try_from(operands).map_err(|_| {
                invalid(
                    &format!("{field}.subtract"),
                    "subtract requires exactly two operands",
                )
            })?;
            Ok(Expression::Subtract(Box::new(operands)))
        }
        operator => Err(invalid(
            field,
            &format!("unsupported numeric expression `{operator}`"),
        )),
    }
}

fn parse_operands(
    value: &Value,
    field: &str,
    minimum: usize,
    exact: Option<usize>,
) -> Result<Vec<Expression>> {
    let operands = value
        .as_array()
        .filter(|operands| {
            exact.map_or(operands.len() >= minimum, |length| operands.len() == length)
        })
        .ok_or_else(|| {
            exact.map_or_else(
                || invalid(field, &format!("requires at least {minimum} operands")),
                |length| invalid(field, &format!("requires exactly {length} operands")),
            )
        })?;
    operands
        .iter()
        .enumerate()
        .map(|(index, operand)| parse_expression(operand, &format!("{field}.{index}")))
        .collect()
}

fn require_number(
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
    if scalar::kind(catalog, resolved.schema) != Some(scalar::Kind::Number) {
        return Err(invalid(
            field,
            &format!("path `{pointer}` must resolve to a numeric value"),
        ));
    }
    if required && !resolved.required {
        return Err(invalid(
            field,
            &format!("equation source `{pointer}` must be required by its schema"),
        ));
    }
    if required && scalar::accepts(catalog, resolved.schema, &Value::Null) {
        return Err(invalid(
            field,
            &format!("equation source `{pointer}` must not be nullable"),
        ));
    }
    Ok(resolved.schema)
}

impl Expression {
    fn is_well_formed(&self) -> bool {
        match self {
            Self::Path(path) => path::segments(path).is_ok(),
            Self::Add(operands) => operands.len() >= 2 && operands.iter().all(Self::is_well_formed),
            Self::Subtract(operands) => operands.iter().all(Self::is_well_formed),
        }
    }

    fn paths(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        match self {
            Self::Path(path) => Box::new(std::iter::once(path.as_str())),
            Self::Add(operands) => Box::new(operands.iter().flat_map(Self::paths)),
            Self::Subtract(operands) => Box::new(operands.iter().flat_map(Self::paths)),
        }
    }

    fn evaluate(&self, value: &Value) -> Option<scalar::Numeric> {
        match self {
            Self::Path(path) => value.pointer(path).and_then(scalar::Numeric::from_value),
            Self::Add(operands) => operands
                .iter()
                .try_fold(scalar::Numeric::Integer(0), |sum, operand| {
                    sum.add(operand.evaluate(value)?)
                }),
            Self::Subtract(operands) => operands[0]
                .evaluate(value)?
                .subtract(operands[1].evaluate(value)?),
        }
    }
}
