use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::ast::{ContractAst, TypeExpr};
use crate::error::{Error, Result};

pub(crate) fn attach(ast: &ContractAst, schemas: &mut BTreeMap<String, Value>) -> Result<()> {
    let Some(soundness) = ast.declarations.get("SchemaSoundness") else {
        return Ok(());
    };
    let Some(owners) = soundness.properties() else {
        return Err(invalid("SchemaSoundness", "must be an interface"));
    };
    for owner in owners {
        let TypeExpr::Tuple(relations) = &owner.value else {
            return Err(invalid(&owner.name, "constraint list must be a tuple"));
        };
        let constraints = relations
            .iter()
            .map(|relation| lower_relation(&owner.name, relation))
            .collect::<Result<Vec<_>>>()?;
        let schema = schemas
            .get_mut(&owner.name)
            .ok_or_else(|| invalid(&owner.name, "owner payload type was not found"))?;
        let object = schema
            .as_object_mut()
            .ok_or_else(|| invalid(&owner.name, "owner schema is not an object"))?;
        object.insert("x-simile-constraints".to_owned(), Value::Array(constraints));
    }
    Ok(())
}

fn lower_relation(owner_name: &str, relation: &TypeExpr) -> Result<Value> {
    let TypeExpr::Reference { name, arguments } = relation else {
        return Err(invalid(
            owner_name,
            "relationship must be a named semantic type",
        ));
    };
    match name.as_str() {
        "Ordered" => {
            require_arguments(owner_name, name, arguments, 2)?;
            let owner = owner(owner_name, &arguments[0])?;
            Ok(json!({ "ordered": { "paths": paths(owner_name, &owner, &arguments[1])? } }))
        }
        "Bounded" => lower_bounded(owner_name, arguments),
        "Implies" => lower_implies(owner_name, arguments),
        "EqualFields" => {
            require_arguments(owner_name, name, arguments, 2)?;
            let owner = owner(owner_name, &arguments[0])?;
            Ok(json!({ "equal": { "paths": paths(owner_name, &owner, &arguments[1])? } }))
        }
        "DistinctValues" => {
            require_arguments(owner_name, name, arguments, 2)?;
            let owner = owner(owner_name, &arguments[0])?;
            Ok(json!({
                "distinct": {
                    "source": pointer(owner_name, &owner, &arguments[1])?
                }
            }))
        }
        "SetRelationship" => {
            require_arguments(owner_name, name, arguments, 4)?;
            let owner = owner(owner_name, &arguments[0])?;
            Ok(json!({
                "set": {
                    "left": pointer(owner_name, &owner, &arguments[1])?,
                    "relation": literal_string(owner_name, &arguments[2])?,
                    "right": pointer(owner_name, &owner, &arguments[3])?
                }
            }))
        }
        "Equation" => {
            require_arguments(owner_name, name, arguments, 3)?;
            let owner = owner(owner_name, &arguments[0])?;
            Ok(json!({
                "equation": {
                    "target": pointer(owner_name, &owner, &arguments[1])?,
                    "value": expression(owner_name, &owner, &arguments[2])?
                }
            }))
        }
        "Aggregate" => lower_aggregate(owner_name, arguments),
        "Template" => {
            require_arguments(owner_name, name, arguments, 3)?;
            let owner = owner(owner_name, &arguments[0])?;
            Ok(json!({
                "template": {
                    "path": pointer(owner_name, &owner, &arguments[1])?,
                    "value": literal_string(owner_name, &arguments[2])?
                }
            }))
        }
        "Lifecycle" => lower_lifecycle(owner_name, arguments),
        other => Err(invalid(
            owner_name,
            format!("unsupported relationship `{other}`"),
        )),
    }
}

fn lower_bounded(owner_name: &str, arguments: &[TypeExpr]) -> Result<Value> {
    require_arguments(owner_name, "Bounded", arguments, 3)?;
    let owner = owner(owner_name, &arguments[0])?;
    let Some(bounds) = arguments[2].properties() else {
        return Err(invalid(owner_name, "bounded limits must be an object"));
    };
    let mut value = Map::new();
    value.insert(
        "path".to_owned(),
        Value::String(pointer(owner_name, &owner, &arguments[1])?),
    );
    for bound in bounds {
        let TypeExpr::Reference { name, arguments } = &bound.value else {
            return Err(invalid(
                owner_name,
                "bound source must be Field<Owner, Path>",
            ));
        };
        require_arguments(owner_name, name, arguments, 2)?;
        if name != "Field" {
            return Err(invalid(
                owner_name,
                "bound source must be Field<Owner, Path>",
            ));
        }
        let bound_owner = self::owner(owner_name, &arguments[0])?;
        ensure_same_owner(owner_name, &owner, &bound_owner)?;
        value.insert(
            bound.name.clone(),
            json!({ "path": pointer(owner_name, &bound_owner, &arguments[1])? }),
        );
    }
    Ok(json!({ "bounded": value }))
}

fn lower_implies(owner_name: &str, arguments: &[TypeExpr]) -> Result<Value> {
    require_arguments(owner_name, "Implies", arguments, 2)?;
    let owner = predicate_owner(owner_name, &arguments[0])?;
    let TypeExpr::Tuple(consequences) = &arguments[1] else {
        return Err(invalid(
            owner_name,
            "implication consequences must be a tuple",
        ));
    };
    let all = consequences
        .iter()
        .map(|item| predicate(owner_name, &owner, item))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "implies": {
            "when": predicate(owner_name, &owner, &arguments[0])?,
            "then": { "all": all }
        }
    }))
}

fn lower_aggregate(owner_name: &str, arguments: &[TypeExpr]) -> Result<Value> {
    require_arguments(owner_name, "Aggregate", arguments, 5)?;
    let owner = owner(owner_name, &arguments[0])?;
    let mut aggregate = Map::from_iter([
        (
            "target".to_owned(),
            Value::String(pointer(owner_name, &owner, &arguments[1])?),
        ),
        (
            "operation".to_owned(),
            Value::String(literal_string(owner_name, &arguments[2])?.to_owned()),
        ),
        (
            "source".to_owned(),
            Value::String(pointer(owner_name, &owner, &arguments[3])?),
        ),
    ]);
    if !matches!(arguments[4], TypeExpr::Never) {
        aggregate.insert(
            "when".to_owned(),
            predicate(owner_name, &owner, &arguments[4])?,
        );
    }
    Ok(json!({ "aggregate": aggregate }))
}

fn lower_lifecycle(owner_name: &str, arguments: &[TypeExpr]) -> Result<Value> {
    require_arguments(owner_name, "Lifecycle", arguments, 4)?;
    let owner = owner(owner_name, &arguments[0])?;
    let TypeExpr::Tuple(transitions) = &arguments[3] else {
        return Err(invalid(owner_name, "lifecycle transitions must be a tuple"));
    };
    let transitions = transitions
        .iter()
        .map(|transition| {
            let from = required_property(owner_name, transition, "from")?;
            let to = required_property(owner_name, transition, "to")?;
            let at = required_property(owner_name, transition, "at")?;
            Ok(json!({
                "from": literal_or_array(owner_name, from)?,
                "to": literal_or_array(owner_name, to)?,
                "at": pointer(owner_name, &owner, at)?
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "lifecycle": {
            "state": pointer(owner_name, &owner, &arguments[1])?,
            "created-at": pointer(owner_name, &owner, &arguments[2])?,
            "transitions": transitions
        }
    }))
}

fn expression(owner_name: &str, owner: &Owner, value: &TypeExpr) -> Result<Value> {
    let TypeExpr::Reference { name, arguments } = value else {
        return Err(invalid(
            owner_name,
            "equation expression must be a semantic type",
        ));
    };
    match name.as_str() {
        "Field" => {
            require_arguments(owner_name, name, arguments, 2)?;
            let field_owner = self::owner(owner_name, &arguments[0])?;
            ensure_same_owner(owner_name, owner, &field_owner)?;
            Ok(json!({
                "path": pointer(owner_name, &field_owner, &arguments[1])?
            }))
        }
        "Literal" => {
            require_arguments(owner_name, name, arguments, 1)?;
            Ok(json!({ "literal": literal(owner_name, &arguments[0])? }))
        }
        "Add" | "Multiply" => {
            require_arguments(owner_name, name, arguments, 1)?;
            let TypeExpr::Tuple(operands) = &arguments[0] else {
                return Err(invalid(owner_name, "expression operands must be a tuple"));
            };
            let operands = operands
                .iter()
                .map(|item| expression(owner_name, owner, item))
                .collect::<Result<Vec<_>>>()?;
            Ok(json!({ name.to_ascii_lowercase(): operands }))
        }
        "Subtract" => {
            require_arguments(owner_name, name, arguments, 2)?;
            Ok(json!({
                "subtract": [
                    expression(owner_name, owner, &arguments[0])?,
                    expression(owner_name, owner, &arguments[1])?
                ]
            }))
        }
        other => Err(invalid(
            owner_name,
            format!("unsupported equation expression `{other}`"),
        )),
    }
}

fn predicate(owner_name: &str, owner: &Owner, value: &TypeExpr) -> Result<Value> {
    let TypeExpr::Reference { name, arguments } = value else {
        return Err(invalid(owner_name, "predicate must be a semantic type"));
    };
    match name.as_str() {
        "EqualsAt" => {
            require_arguments(owner_name, name, arguments, 3)?;
            ensure_predicate_owner(owner_name, owner, &arguments[0])?;
            Ok(json!({
                "path": pointer(owner_name, owner, &arguments[1])?,
                "equals": literal(owner_name, &arguments[2])?
            }))
        }
        "StateAt" => {
            require_arguments(owner_name, name, arguments, 3)?;
            ensure_predicate_owner(owner_name, owner, &arguments[0])?;
            Ok(json!({
                "path": pointer(owner_name, owner, &arguments[1])?,
                "state": literal_string(owner_name, &arguments[2])?
            }))
        }
        "ContainsAt" => {
            require_arguments(owner_name, name, arguments, 3)?;
            ensure_predicate_owner(owner_name, owner, &arguments[0])?;
            Ok(json!({
                "path": pointer(owner_name, owner, &arguments[1])?,
                "contains": literal(owner_name, &arguments[2])?
            }))
        }
        "CompareAt" => {
            require_arguments(owner_name, name, arguments, 4)?;
            ensure_predicate_owner(owner_name, owner, &arguments[0])?;
            Ok(json!({
                "compare": {
                    "left": { "path": pointer(owner_name, owner, &arguments[1])? },
                    "operator": literal_string(owner_name, &arguments[2])?,
                    "right": { "path": pointer(owner_name, owner, &arguments[3])? }
                }
            }))
        }
        other => Err(invalid(
            owner_name,
            format!("unsupported predicate `{other}`"),
        )),
    }
}

fn predicate_owner(owner_name: &str, value: &TypeExpr) -> Result<Owner> {
    let TypeExpr::Reference { arguments, .. } = value else {
        return Err(invalid(owner_name, "predicate must be a semantic type"));
    };
    arguments
        .first()
        .ok_or_else(|| invalid(owner_name, "predicate owner is missing"))
        .and_then(|value| owner(owner_name, value))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Owner {
    prefix: Vec<String>,
}

fn ensure_predicate_owner(owner_name: &str, expected: &Owner, value: &TypeExpr) -> Result<()> {
    let actual = owner(owner_name, value)?;
    ensure_same_owner(owner_name, expected, &actual)
}

fn ensure_same_owner(owner_name: &str, expected: &Owner, actual: &Owner) -> Result<()> {
    if expected == actual {
        Ok(())
    } else {
        Err(invalid(
            owner_name,
            "relationship members must use the same selected owner",
        ))
    }
}

fn owner(owner_name: &str, value: &TypeExpr) -> Result<Owner> {
    let TypeExpr::Reference { name, arguments } = value else {
        return Err(invalid(
            owner_name,
            "constraint owner must be a payload type",
        ));
    };
    match name.as_str() {
        "Select" => {
            require_arguments(owner_name, name, arguments, 2)?;
            let mut selected = owner(owner_name, &arguments[0])?;
            selected.prefix.extend(path(owner_name, &arguments[1])?);
            Ok(selected)
        }
        "Readonly" | "NonNullable" => arguments
            .first()
            .ok_or_else(|| invalid(owner_name, "constraint owner wrapper is empty"))
            .and_then(|value| owner(owner_name, value)),
        _ if name == owner_name => Ok(Owner { prefix: Vec::new() }),
        _ => Err(invalid(
            owner_name,
            format!("constraint owner `{name}` does not match `{owner_name}`"),
        )),
    }
}

fn pointer(owner_name: &str, owner: &Owner, value: &TypeExpr) -> Result<String> {
    let mut segments = owner.prefix.clone();
    segments.extend(path(owner_name, value)?);
    Ok(format!(
        "/{}",
        segments
            .iter()
            .map(|segment| segment.replace('~', "~0").replace('/', "~1"))
            .collect::<Vec<_>>()
            .join("/")
    ))
}

fn paths(owner_name: &str, owner: &Owner, value: &TypeExpr) -> Result<Vec<String>> {
    let TypeExpr::Tuple(values) = value else {
        return Err(invalid(owner_name, "paths must be a tuple"));
    };
    values
        .iter()
        .map(|value| pointer(owner_name, owner, value))
        .collect()
}

fn path(owner_name: &str, value: &TypeExpr) -> Result<Vec<String>> {
    let TypeExpr::Tuple(segments) = value else {
        return Err(invalid(owner_name, "field path must be a tuple"));
    };
    segments
        .iter()
        .map(|segment| literal_string(owner_name, segment).map(str::to_owned))
        .collect()
}

fn literal_or_array(owner_name: &str, value: &TypeExpr) -> Result<Value> {
    match value {
        TypeExpr::Tuple(items) => items
            .iter()
            .map(|item| literal(owner_name, item))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        _ => literal(owner_name, value),
    }
}

fn literal(owner_name: &str, value: &TypeExpr) -> Result<Value> {
    match value {
        TypeExpr::Literal(value) => Ok(value.clone()),
        TypeExpr::Null => Ok(Value::Null),
        _ => Err(invalid(owner_name, "expected a literal type")),
    }
}

fn literal_string<'a>(owner_name: &str, value: &'a TypeExpr) -> Result<&'a str> {
    value
        .literal_string()
        .ok_or_else(|| invalid(owner_name, "expected a string literal type"))
}

fn required_property<'a>(
    owner_name: &str,
    value: &'a TypeExpr,
    name: &str,
) -> Result<&'a TypeExpr> {
    value
        .property(name)
        .ok_or_else(|| invalid(owner_name, format!("missing `{name}`")))
}

fn require_arguments(
    owner_name: &str,
    name: &str,
    arguments: &[TypeExpr],
    count: usize,
) -> Result<()> {
    if arguments.len() == count {
        Ok(())
    } else {
        Err(invalid(
            owner_name,
            format!("{name} requires {count} type arguments"),
        ))
    }
}

fn invalid(owner: &str, detail: impl Into<String>) -> Error {
    Error::InvalidConstraint {
        owner: owner.to_owned(),
        detail: detail.into(),
    }
}
