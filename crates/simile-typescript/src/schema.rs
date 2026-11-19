use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};

use crate::ast::{ContractAst, TypeExpr};
use crate::constraints;
use crate::error::Result;

pub(crate) fn components(ast: &ContractAst) -> Result<BTreeMap<String, Value>> {
    let mut schemas = ast
        .declarations
        .iter()
        .filter(|(name, _)| is_payload_declaration(name))
        .map(|(name, value)| Ok((name.clone(), type_schema(ast, value)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    constraints::attach(ast, &mut schemas)?;
    Ok(schemas)
}

pub(crate) fn type_schema(ast: &ContractAst, value: &TypeExpr) -> Result<Value> {
    Ok(match value {
        TypeExpr::Any => json!({}),
        TypeExpr::Never => Value::Bool(false),
        TypeExpr::Null => json!({ "type": "null" }),
        TypeExpr::Boolean => json!({ "type": "boolean" }),
        TypeExpr::Number => json!({ "type": "number" }),
        TypeExpr::String => json!({ "type": "string" }),
        TypeExpr::Literal(literal) => json!({ "const": literal }),
        TypeExpr::Template(parts) => json!({
            "type": "string",
            "pattern": template_pattern(parts)
        }),
        TypeExpr::Array(item) => json!({
            "type": "array",
            "items": type_schema(ast, item)?
        }),
        TypeExpr::Tuple(items) => json!({
            "type": "array",
            "prefixItems": items
                .iter()
                .map(|item| type_schema(ast, item))
                .collect::<Result<Vec<_>>>()?,
            "minItems": items.len(),
            "maxItems": items.len()
        }),
        TypeExpr::Object {
            properties,
            additional,
        } => {
            let mut schemas = Map::new();
            let required = properties
                .iter()
                .filter(|property| !property.optional)
                .map(|property| Value::String(property.name.clone()))
                .collect::<Vec<_>>();
            for property in properties {
                schemas.insert(property.name.clone(), type_schema(ast, &property.value)?);
            }
            let additional = additional
                .as_deref()
                .map(|schema| type_schema(ast, schema))
                .transpose()?
                // TypeScript object types are structurally open. Exact payload objects use the
                // explicit `Closed<T>` semantic wrapper so partial proof refinements can compose.
                .unwrap_or(Value::Bool(true));
            let mut object = Map::from_iter([
                ("type".to_owned(), Value::String("object".to_owned())),
                ("properties".to_owned(), Value::Object(schemas)),
                ("additionalProperties".to_owned(), additional),
            ]);
            if !required.is_empty() {
                object.insert("required".to_owned(), Value::Array(required));
            }
            Value::Object(object)
        }
        TypeExpr::Reference { name, arguments } => reference_schema(ast, name, arguments)?,
        TypeExpr::Union(items) => {
            let alternatives = items
                .iter()
                .filter(|item| !matches!(item, TypeExpr::Never))
                .map(|item| type_schema(ast, item))
                .collect::<Result<Vec<_>>>()?;
            match alternatives.as_slice() {
                [] => Value::Bool(false),
                [single] => single.clone(),
                _ => json!({ "oneOf": alternatives }),
            }
        }
        TypeExpr::Intersection(items) => json!({
            "allOf": items
                .iter()
                .map(|item| type_schema(ast, item))
                .collect::<Result<Vec<_>>>()?
        }),
    })
}

pub(crate) fn literal_value(ast: &ContractAst, value: &TypeExpr) -> Option<Value> {
    literal_value_inner(ast, value, &mut BTreeSet::new())
}

fn literal_value_inner(
    ast: &ContractAst,
    value: &TypeExpr,
    resolving: &mut BTreeSet<String>,
) -> Option<Value> {
    match value {
        TypeExpr::Null => Some(Value::Null),
        TypeExpr::Literal(value) => Some(value.clone()),
        TypeExpr::Tuple(items) => items
            .iter()
            .map(|item| literal_value_inner(ast, item, resolving))
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        TypeExpr::Object { properties, .. } => {
            let mut object = Map::new();
            for property in properties {
                let value = literal_value_inner(ast, &property.value, resolving)?;
                object.insert(property.name.clone(), value);
            }
            Some(Value::Object(object))
        }
        TypeExpr::Intersection(items) => {
            let mut object = Map::new();
            for item in items {
                let Value::Object(part) = literal_value_inner(ast, item, resolving)? else {
                    return None;
                };
                object.extend(part);
            }
            Some(Value::Object(object))
        }
        TypeExpr::Reference { name, arguments }
            if matches!(name.as_str(), "Readonly" | "NonNullable" | "Closed") =>
        {
            arguments
                .first()
                .and_then(|item| literal_value_inner(ast, item, resolving))
        }
        TypeExpr::Reference { name, .. } => {
            if !resolving.insert(name.clone()) {
                return None;
            }
            let result = ast
                .declarations
                .get(name)
                .and_then(|item| literal_value_inner(ast, item, resolving));
            resolving.remove(name);
            result
        }
        _ => None,
    }
}

fn reference_schema(ast: &ContractAst, name: &str, arguments: &[TypeExpr]) -> Result<Value> {
    if let Some(detail) = ast.unsupported.get(name) {
        return Err(crate::error::Error::Unsupported {
            name: name.to_owned(),
            detail: detail.clone(),
        });
    }
    Ok(match name {
        "DateTime" => json!({ "type": "string", "format": "date-time" }),
        "Uri" => json!({ "type": "string", "format": "uri" }),
        "EmailAddress" => json!({ "type": "string", "format": "email" }),
        "Binary" | "Uint8Array" => json!({ "type": "string", "format": "binary" }),
        "Readonly" | "NonNullable" => arguments
            .first()
            .map(|value| type_schema(ast, value))
            .transpose()?
            .unwrap_or_else(|| json!({})),
        "Closed" => {
            let mut schema = arguments
                .first()
                .map(|value| type_schema(ast, value))
                .transpose()?
                .unwrap_or_else(|| json!({}));
            if let Some(object) = schema.as_object_mut() {
                object.insert("additionalProperties".to_owned(), Value::Bool(false));
            }
            schema
        }
        "Record" => json!({
            "type": "object",
            "additionalProperties": arguments
                .get(1)
                .map(|value| type_schema(ast, value))
                .transpose()?
                .unwrap_or_else(|| json!({}))
        }),
        "Exclude" => {
            let base = arguments
                .first()
                .map(|value| type_schema(ast, value))
                .transpose()?
                .unwrap_or_else(|| json!({}));
            let excluded = arguments
                .get(1)
                .map(|value| type_schema(ast, value))
                .transpose()?
                .unwrap_or_else(|| json!({}));
            json!({ "allOf": [base, { "not": excluded }] })
        }
        _ if ast.declarations.contains_key(name) => {
            json!({ "$ref": format!("#/components/schemas/{name}") })
        }
        _ => json!({}),
    })
}

fn is_payload_declaration(name: &str) -> bool {
    !matches!(
        name,
        "CollectionVersion"
            | "OperationCount"
            | "SchemaCount"
            | "ConstraintCount"
            | "ResponseRelationCount"
            | "ServiceName"
            | "OperationExamples"
            | "ResponseRelations"
            | "SchemaSoundness"
            | "Operations"
    ) && !name.ends_with("Operations")
        && !name.starts_with("OperationExamples.")
}

fn template_pattern(parts: &[String]) -> String {
    let mut pattern = String::from("^");
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            pattern.push_str(".*");
        }
        escape_pattern(part, &mut pattern);
    }
    pattern.push('$');
    pattern
}

fn escape_pattern(value: &str, target: &mut String) {
    for character in value.chars() {
        if matches!(
            character,
            '.' | '+' | '*' | '?' | '^' | '$' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\'
        ) {
            target.push('\\');
        }
        target.push(character);
    }
}
