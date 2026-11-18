//! Decodes the public extension syntax into the compact relation model.

use serde_json::Value;

use super::{Condition, Derivation, Expression, Mapping, Rule};
use crate::soundness::path;
use crate::{CompileError, Result};

pub(super) fn definition(
    definition: &Value,
    field: &str,
) -> Result<(Vec<Mapping>, Vec<Derivation>, Vec<Rule>)> {
    let parsed = if definition.is_array() {
        (legacy_mappings(definition, field)?, Vec::new(), Vec::new())
    } else {
        let object = definition
            .as_object()
            .ok_or_else(|| invalid(field, "extension must be an object"))?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "bindings" | "rules"))
        {
            return Err(invalid(
                field,
                "extension supports only `bindings` and `rules`",
            ));
        }
        let (mappings, derivations) = bindings(object.get("bindings"), field)?;
        (mappings, derivations, rules(object.get("rules"), field)?)
    };
    if parsed.0.is_empty() && parsed.1.is_empty() && parsed.2.is_empty() {
        return Err(invalid(field, "extension must declare a binding or rule"));
    }
    Ok(parsed)
}

fn legacy_mappings(value: &Value, field: &str) -> Result<Vec<Mapping>> {
    value
        .as_array()
        .ok_or_else(|| invalid(field, "mappings must be an array"))?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let field = format!("{field}.{index}");
            let object = value
                .as_object()
                .ok_or_else(|| invalid(&field, "mapping must be an object"))?;
            if object.len() != 2 || !object.contains_key("source") || !object.contains_key("target")
            {
                return Err(invalid(
                    &field,
                    "mapping requires exactly `source` and `target`",
                ));
            }
            Ok(Mapping {
                source: selector(object.get("source"), &format!("{field}.source"))?,
                target: pointer(object.get("target"), &format!("{field}.target"))?,
                target_schema: 0,
                default: None,
            })
        })
        .collect()
}

fn bindings(value: Option<&Value>, field: &str) -> Result<(Vec<Mapping>, Vec<Derivation>)> {
    let Some(value) = value else {
        return Ok((Vec::new(), Vec::new()));
    };
    let values = value
        .as_array()
        .ok_or_else(|| invalid(&format!("{field}.bindings"), "bindings must be an array"))?;
    let mut mappings = Vec::new();
    let mut derivations = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let item = format!("{field}.bindings.{index}");
        let object = value
            .as_object()
            .ok_or_else(|| invalid(&item, "binding must be an object"))?;
        let target = pointer(object.get("to"), &format!("{item}.to"))?;
        match (object.get("from"), object.get("value")) {
            (Some(source), None) => mappings.push(Mapping {
                source: selector(Some(source), &format!("{item}.from"))?,
                target,
                target_schema: 0,
                default: object.get("default").cloned(),
            }),
            (None, Some(value)) if object.len() == 2 => derivations.push(Derivation {
                target,
                value: expression(value, &format!("{item}.value"))?,
            }),
            _ => {
                return Err(invalid(
                    &item,
                    "binding requires exactly one of `from` or `value`",
                ));
            }
        }
    }
    Ok((mappings, derivations))
}

fn expression(value: &Value, field: &str) -> Result<Expression> {
    let object = value
        .as_object()
        .filter(|object| object.len() == 1)
        .ok_or_else(|| invalid(field, "expression requires exactly one operator"))?;
    let (operator, value) = object
        .iter()
        .next()
        .ok_or_else(|| invalid(field, "expression operator is required"))?;
    match operator.as_str() {
        "request" => selector(Some(value), field).map(Expression::Request),
        "path" => selector(Some(value), field).map(Expression::Relative),
        "literal" => Ok(Expression::Literal(value.clone())),
        "add" | "multiply" => {
            let values = value
                .as_array()
                .filter(|values| values.len() >= 2)
                .ok_or_else(|| invalid(field, "numeric operation requires at least two values"))?
                .iter()
                .map(|value| expression(value, field))
                .collect::<Result<Vec<_>>>()?;
            if operator == "add" {
                Ok(Expression::Add(values))
            } else {
                Ok(Expression::Multiply(values))
            }
        }
        "sum" => {
            let sum = value
                .as_object()
                .filter(|sum| sum.len() == 2)
                .ok_or_else(|| invalid(field, "sum requires exactly `each` and `value`"))?;
            Ok(Expression::Sum {
                each: selector(sum.get("each"), field)?,
                value: Box::new(expression(
                    sum.get("value")
                        .ok_or_else(|| invalid(field, "sum value is required"))?,
                    field,
                )?),
            })
        }
        _ => Err(invalid(field, "unsupported expression operator")),
    }
}

fn rules(value: Option<&Value>, field: &str) -> Result<Vec<Rule>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| invalid(&format!("{field}.rules"), "rules must be an array"))?
        .iter()
        .enumerate()
        .map(|(index, value)| rule(value, &format!("{field}.rules.{index}")))
        .collect()
}

fn rule(value: &Value, field: &str) -> Result<Rule> {
    let object = value
        .as_object()
        .filter(|object| object.len() == 2)
        .ok_or_else(|| invalid(field, "rule requires exactly `when` and `then`"))?;
    let when = object
        .get("when")
        .and_then(Value::as_object)
        .and_then(|when| when.get("all"))
        .and_then(Value::as_array)
        .filter(|conditions| !conditions.is_empty())
        .ok_or_else(|| invalid(field, "when requires a non-empty `all` array"))?
        .iter()
        .map(|value| condition(value, field))
        .collect::<Result<Vec<_>>>()?;
    let then = object
        .get("then")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid(field, "then must be an object"))?;
    let set: Vec<(String, Value)> = then
        .get("set")
        .map(|set| {
            set.as_object()
                .ok_or_else(|| invalid(field, "set must be an object"))?
                .iter()
                .map(|(target, value)| {
                    path::segments(target).map_err(|reason| invalid(field, &reason))?;
                    Ok((target.clone(), value.clone()))
                })
                .collect()
        })
        .transpose()?
        .unwrap_or_default();
    let omit: Vec<String> = then
        .get("omit")
        .map(|omit| {
            omit.as_array()
                .ok_or_else(|| invalid(field, "omit must be an array"))?
                .iter()
                .map(|value| pointer(Some(value), field))
                .collect()
        })
        .transpose()?
        .unwrap_or_default();
    if set.is_empty() && omit.is_empty() {
        return Err(invalid(field, "then must set or omit a response field"));
    }
    Ok(Rule { when, set, omit })
}

fn condition(value: &Value, field: &str) -> Result<Condition> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid(field, "condition must be an object"))?;
    Ok(Condition {
        path: selector(object.get("request"), field)?,
        equals: object
            .get("equals")
            .cloned()
            .ok_or_else(|| invalid(field, "condition requires `equals`"))?,
        default: object.get("default").cloned(),
    })
}

fn pointer(value: Option<&Value>, field: &str) -> Result<String> {
    let value = value
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(field, "value must be an absolute JSON Pointer"))?;
    path::segments(value).map_err(|reason| invalid(field, &reason))?;
    Ok(value.to_owned())
}

fn selector(value: Option<&Value>, field: &str) -> Result<String> {
    let value = value
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(field, "value must be a JSON Pointer selector"))?;
    path::resolve_selector_syntax(value).map_err(|reason| invalid(field, &reason))?;
    Ok(value.to_owned())
}

fn invalid(field: &str, reason: &str) -> CompileError {
    CompileError::InvalidConstraint {
        field: field.to_owned(),
        reason: reason.to_owned(),
    }
}
