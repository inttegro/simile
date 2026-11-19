use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};

use crate::error::{Error, Result};
use crate::model::{Member, Model, Shape, ShapeKind, Traits, local_name};

pub(crate) fn components(model: &Model) -> Result<std::collections::BTreeMap<String, Value>> {
    let mut schemas = model
        .shapes
        .iter()
        .filter(|(_, shape)| {
            !matches!(
                shape.kind,
                ShapeKind::Service { .. } | ShapeKind::Operation { .. }
            )
        })
        .map(|(name, shape)| Ok((name.clone(), shape_schema(model, shape)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    materialize_discriminator_members(&mut schemas)?;
    Ok(schemas)
}

pub(crate) fn member_schema(model: &Model, member: &Member) -> Result<Value> {
    let mut schema = reference_or_prelude(model, &member.target);
    apply_traits(&mut schema, &member.traits)?;
    Ok(schema)
}

pub(crate) fn examples(traits: &Traits) -> Vec<Value> {
    traits
        .get("dataExamples")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("json").cloned())
        .collect()
}

fn shape_schema(model: &Model, shape: &Shape) -> Result<Value> {
    let mut schema = match &shape.kind {
        ShapeKind::Structure { members, mixins } => {
            let mut properties = Map::new();
            let mut required = Vec::new();
            for member in merged_structure_members(model, members, mixins, &mut BTreeSet::new())? {
                let name = member
                    .traits
                    .get("jsonName")
                    .and_then(Value::as_str)
                    .unwrap_or(&member.name)
                    .to_owned();
                if !member.target.is_empty() {
                    properties.insert(name.clone(), member_schema(model, &member)?);
                }
                if member.traits.contains("required") {
                    required.push(Value::String(name));
                }
            }
            let mut object = Map::from_iter([
                ("type".to_owned(), Value::String("object".to_owned())),
                ("properties".to_owned(), Value::Object(properties)),
                ("additionalProperties".to_owned(), Value::Bool(false)),
            ]);
            if !required.is_empty() {
                object.insert("required".to_owned(), Value::Array(required));
            }
            Value::Object(object)
        }
        ShapeKind::Union { members } => {
            let untagged = shape.traits.contains("untagged");
            let alternatives = members
                .iter()
                .map(|member| {
                    if untagged {
                        member_schema(model, member)
                    } else {
                        Ok(json!({
                            "type": "object",
                            "properties": { member.name.clone(): member_schema(model, member)? },
                            "required": [member.name.clone()],
                            "additionalProperties": false
                        }))
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            json!({ "oneOf": alternatives })
        }
        ShapeKind::List { member } => {
            json!({ "type": "array", "items": member_schema(model, member)? })
        }
        ShapeKind::Map { value } => {
            json!({ "type": "object", "additionalProperties": member_schema(model, value)? })
        }
        ShapeKind::Simple { smithy_type } => prelude_schema(smithy_type),
        ShapeKind::Enum { integer, values } => json!({
            "type": if *integer { "integer" } else { "string" },
            "enum": values
        }),
        ShapeKind::Service { .. } | ShapeKind::Operation { .. } => Value::Object(Map::new()),
    };
    apply_traits(&mut schema, &shape.traits)?;
    Ok(schema)
}

fn merged_structure_members(
    model: &Model,
    members: &[Member],
    mixins: &[String],
    visiting: &mut BTreeSet<String>,
) -> Result<Vec<Member>> {
    let mut merged = BTreeMap::<String, Member>::new();
    for mixin_name in mixins {
        let local = local_name(mixin_name);
        if !visiting.insert(local.to_owned()) {
            return Err(Error::Invalid {
                context: "mixin",
                detail: format!("cyclic mixin inheritance through `{local}`"),
            });
        }
        let mixin = model.shape(local)?;
        let ShapeKind::Structure {
            members: mixin_members,
            mixins: inherited_mixins,
        } = &mixin.kind
        else {
            return Err(Error::Invalid {
                context: "mixin",
                detail: format!("`{local}` is not a structure"),
            });
        };
        for member in merged_structure_members(model, mixin_members, inherited_mixins, visiting)? {
            merged.insert(member.name.clone(), member);
        }
        visiting.remove(local);
    }
    for member in members {
        if member.target.is_empty() {
            let inherited = merged.get_mut(&member.name).ok_or_else(|| Error::Invalid {
                context: "mixin member",
                detail: format!("applied traits target unknown member `{}`", member.name),
            })?;
            inherited.traits.extend(member.traits.clone());
        } else {
            merged.insert(member.name.clone(), member.clone());
        }
    }
    Ok(merged.into_values().collect())
}

fn reference_or_prelude(model: &Model, target: &str) -> Value {
    let local = local_name(target);
    if model.shapes.contains_key(local) {
        reference(local)
    } else {
        prelude_schema(local)
    }
}

fn reference(name: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{name}") })
}

fn prelude_schema(name: &str) -> Value {
    match local_name(name) {
        "String" => json!({ "type": "string" }),
        "Boolean" => json!({ "type": "boolean" }),
        "Byte" | "Short" | "Integer" | "Long" | "BigInteger" => {
            json!({ "type": "integer" })
        }
        "Float" | "Double" | "BigDecimal" => json!({ "type": "number" }),
        "Timestamp" => json!({ "type": "string", "format": "date-time" }),
        "Blob" => json!({ "type": "string", "format": "binary" }),
        "Document" | "Unit" | "" => Value::Object(Map::new()),
        _ => Value::Object(Map::new()),
    }
}

fn apply_traits(schema: &mut Value, traits: &Traits) -> Result<()> {
    if let Some(source_schema) = traits
        .get("openapiExtensions")
        .and_then(Value::as_object)
        .and_then(|extensions| extensions.get("x-inttegro-openapi-source-schema"))
    {
        // Why: Smithy cannot represent every OpenAPI composition/nullability rule. The published
        // projection deliberately carries the exact public fragment so adapters do not silently
        // widen the contract or make request-to-response relations impossible to prove.
        *schema = source_schema.clone();
    }
    let object = ensure_object(schema);
    if let Some(pattern) = traits.get("pattern").and_then(Value::as_str) {
        object.insert("pattern".to_owned(), Value::String(pattern.to_owned()));
    }
    apply_min_max(object, traits.get("length"), "minLength", "maxLength");
    apply_min_max(object, traits.get("range"), "minimum", "maximum");
    if traits.contains("uniqueItems") {
        object.insert("uniqueItems".to_owned(), Value::Bool(true));
    }
    if traits.contains("readonly") {
        object.insert("readOnly".to_owned(), Value::Bool(true));
    }
    if let Some(documentation) = traits.get("documentation").and_then(Value::as_str) {
        object.insert(
            "description".to_owned(),
            Value::String(documentation.to_owned()),
        );
    }
    let values = examples(traits);
    if !values.is_empty() {
        object.insert("examples".to_owned(), Value::Array(values));
    }
    if let Some(extensions) = traits.get("openapiExtensions").and_then(Value::as_object) {
        for (name, value) in extensions {
            if name == "x-inttegro-openapi-source-schema" {
                continue;
            }
            object.insert(name.clone(), value.clone());
        }
    }
    Ok(())
}

fn materialize_discriminator_members(schemas: &mut BTreeMap<String, Value>) -> Result<()> {
    let mut mappings = Vec::new();
    for schema in schemas.values() {
        collect_discriminator_mappings(schema, &mut mappings)?;
    }
    for (shape_name, property, value) in mappings {
        let schema = schemas
            .get_mut(&shape_name)
            .ok_or_else(|| Error::UnknownShape(shape_name.clone()))?;
        let object = schema.as_object_mut().ok_or_else(|| {
            invalid(format!(
                "discriminator target `{shape_name}` is not an object schema"
            ))
        })?;
        let properties = object
            .entry("properties")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| invalid(format!("`{shape_name}` properties must be an object")))?;
        let discriminator_schema = json!({ "type": "string", "enum": [value] });
        if let Some(existing) = properties.get(&property) {
            if existing != &discriminator_schema {
                return Err(invalid(format!(
                    "discriminator `{property}` conflicts with `{shape_name}`"
                )));
            }
        } else {
            properties.insert(property.clone(), discriminator_schema);
        }
        let required = object
            .entry("required")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| invalid(format!("`{shape_name}` required must be an array")))?;
        if !required.iter().any(|name| name.as_str() == Some(&property)) {
            required.push(Value::String(property));
        }
    }
    Ok(())
}

fn collect_discriminator_mappings(
    schema: &Value,
    target: &mut Vec<(String, String, String)>,
) -> Result<()> {
    match schema {
        Value::Array(values) => {
            for value in values {
                collect_discriminator_mappings(value, target)?;
            }
        }
        Value::Object(object) => {
            if let Some(discriminator) = object.get("discriminator").and_then(Value::as_object) {
                let property = discriminator
                    .get("propertyName")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("discriminator is missing propertyName"))?;
                let mapping = discriminator
                    .get("mapping")
                    .and_then(Value::as_object)
                    .ok_or_else(|| invalid("discriminator is missing an explicit mapping"))?;
                for (value, reference) in mapping {
                    let reference = reference.as_str().ok_or_else(|| {
                        invalid("discriminator mapping values must be references")
                    })?;
                    let shape_name =
                        reference
                            .strip_prefix("#/components/schemas/")
                            .ok_or_else(|| {
                                invalid("discriminator mappings must use local references")
                            })?;
                    target.push((shape_name.to_owned(), property.to_owned(), value.to_owned()));
                }
            }
            for value in object.values() {
                collect_discriminator_mappings(value, target)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn invalid(detail: impl Into<String>) -> Error {
    Error::Invalid {
        context: "discriminator",
        detail: detail.into(),
    }
}

fn ensure_object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    match value {
        Value::Object(object) => object,
        _ => unreachable!("value was replaced with an object"),
    }
}

fn apply_min_max(
    target: &mut Map<String, Value>,
    value: Option<&Value>,
    minimum: &str,
    maximum: &str,
) {
    let Some(bounds) = value.and_then(Value::as_object) else {
        return;
    };
    if let Some(value) = bounds.get("min") {
        target.insert(minimum.to_owned(), value.clone());
    }
    if let Some(value) = bounds.get("max") {
        target.insert(maximum.to_owned(), value.clone());
    }
}
