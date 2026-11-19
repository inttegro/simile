use std::collections::{BTreeMap, HashMap};

use serde_json::{Map, Value};
use simile_spec::{
    ApiContract, HttpMethod, MediaTypeContract, OpenApiVersion, Operation, OperationKey,
    ParameterContract, ParameterLocation, RequestBodyContract, ResponseBodyContract,
    ResponseContract, ResponseStatus, ServiceInfo,
};

use crate::{Error, Result};

pub(crate) fn contract(
    document: &Value,
    version: OpenApiVersion,
    supports_extended_methods: bool,
) -> Result<ApiContract> {
    let root = object(document, "document")?;
    let info = object(required(root, "info")?, "info")?;
    let info = ServiceInfo {
        title: string(required(info, "title")?, "info.title")?.to_owned(),
        version: string(required(info, "version")?, "info.version")?.to_owned(),
        description: optional_string(info, "description", "info.description")?,
    };
    let paths = object(required(root, "paths")?, "paths")?;
    let mut operations = BTreeMap::new();
    let mut operation_ids: HashMap<String, OperationKey> = HashMap::new();

    for (path, path_value) in paths {
        let path_item = object(path_value, &format!("paths.{path}"))?;
        let mut entries =
            operation_entries(path_item, supports_extended_methods).collect::<Vec<_>>();
        if supports_extended_methods {
            entries.extend(additional_operation_entries(path_item)?);
        }
        for (method_name, method, value) in entries {
            let operation = operation(document, path, path_item, method_name, method, value)?;
            register_operation_id(&mut operation_ids, &operation)?;
            operations.insert(operation.key.clone(), operation);
        }
    }

    Ok(ApiContract {
        openapi: version,
        info,
        operations,
        schemas: component_schemas(root)?,
    })
}

fn operation_entries(
    path_item: &Map<String, Value>,
    supports_extended_methods: bool,
) -> impl Iterator<Item = (&str, HttpMethod, &Value)> {
    path_item.iter().filter_map(move |(name, value)| {
        method(name, supports_extended_methods).map(|method| (name.as_str(), method, value))
    })
}

fn register_operation_id(
    operation_ids: &mut HashMap<String, OperationKey>,
    operation: &Operation,
) -> Result<()> {
    let Some(operation_id) = &operation.operation_id else {
        return Ok(());
    };
    let Some(first) = operation_ids.insert(operation_id.clone(), operation.key.clone()) else {
        return Ok(());
    };
    Err(Error::DuplicateOperationId {
        operation_id: operation_id.clone(),
        first: first.to_string(),
        second: operation.key.to_string(),
    })
}

fn additional_operation_entries(
    path_item: &Map<String, Value>,
) -> Result<Vec<(&str, HttpMethod, &Value)>> {
    let Some(additional) = path_item.get("additionalOperations") else {
        return Ok(Vec::new());
    };
    let additional = object(additional, "additionalOperations")?;
    additional
        .iter()
        .map(|(name, value)| {
            if method(&name.to_ascii_lowercase(), true).is_some() {
                return Err(Error::DuplicateStandardMethod {
                    method: name.clone(),
                });
            }
            Ok((name.as_str(), HttpMethod::Additional(name.clone()), value))
        })
        .collect()
}

fn method(value: &str, supports_extended_methods: bool) -> Option<HttpMethod> {
    match value {
        "get" => Some(HttpMethod::Get),
        "put" => Some(HttpMethod::Put),
        "post" => Some(HttpMethod::Post),
        "delete" => Some(HttpMethod::Delete),
        "options" => Some(HttpMethod::Options),
        "head" => Some(HttpMethod::Head),
        "patch" => Some(HttpMethod::Patch),
        "trace" => Some(HttpMethod::Trace),
        "query" if supports_extended_methods => Some(HttpMethod::Query),
        _ => None,
    }
}

fn operation(
    document: &Value,
    path: &str,
    path_item: &Map<String, Value>,
    method_name: &str,
    method: HttpMethod,
    value: &Value,
) -> Result<Operation> {
    let field = format!("paths.{path}.{method_name}");
    let body = object(value, &field)?;
    let operation_id = optional_string(body, "operationId", "operationId")?;
    let summary = optional_string(body, "summary", "summary")?;
    let tags = body
        .get("tags")
        .map(|tags| string_list(tags, &format!("{field}.tags")))
        .transpose()?
        .unwrap_or_default();
    let responses = body
        .get("responses")
        .ok_or_else(|| Error::MissingResponses {
            method: method.to_string(),
            path: path.to_owned(),
        })?;
    let responses = object(responses, "responses")?;
    if responses.is_empty() {
        return Err(Error::MissingResponses {
            method: method.to_string(),
            path: path.to_owned(),
        });
    }
    let responses = responses
        .iter()
        .map(|(status, definition)| {
            let response_field = format!("{field}.responses.{status}");
            Ok(ResponseContract {
                status: response_status(status).ok_or_else(|| Error::InvalidResponseStatus {
                    method: method.to_string(),
                    path: path.to_owned(),
                    status: status.clone(),
                })?,
                definition: definition.clone(),
                body: response_body(document, definition, &response_field)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let raw_request_body = body.get("requestBody").cloned();
    let request = body
        .get("requestBody")
        .map(|value| request_body(document, value, &format!("{field}.requestBody")))
        .transpose()?;
    let parameters = operation_parameters(document, path_item, body, &field)?;

    Ok(Operation {
        key: OperationKey::new(&method, path),
        operation_id,
        method,
        path: path.to_owned(),
        summary,
        tags,
        parameters,
        request_body: raw_request_body,
        request,
        responses,
    })
}

fn operation_parameters(
    document: &Value,
    path_item: &Map<String, Value>,
    operation: &Map<String, Value>,
    field: &str,
) -> Result<Vec<ParameterContract>> {
    let mut parameters = BTreeMap::new();
    collect_parameters(
        document,
        path_item.get("parameters"),
        &format!("{field}.pathParameters"),
        &mut parameters,
    )?;
    collect_parameters(
        document,
        operation.get("parameters"),
        &format!("{field}.parameters"),
        &mut parameters,
    )?;
    Ok(parameters.into_values().collect())
}

fn collect_parameters(
    document: &Value,
    value: Option<&Value>,
    field: &str,
    parameters: &mut BTreeMap<(ParameterLocation, String), ParameterContract>,
) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    let values = value.as_array().ok_or_else(|| Error::InvalidField {
        field: field.to_owned(),
        expected: "an array",
    })?;
    for (index, value) in values.iter().enumerate() {
        let parameter_field = format!("{field}.{index}");
        let value = resolve_local_reference(document, value, &parameter_field, 0)?;
        let object = object(value, &parameter_field)?;
        let name = string(
            required(object, "name")?,
            &format!("{parameter_field}.name"),
        )?
        .to_owned();
        let location = parameter_location(string(
            required(object, "in")?,
            &format!("{parameter_field}.in"),
        )?)
        .ok_or_else(|| Error::InvalidField {
            field: format!("{parameter_field}.in"),
            expected: "one of path, query, header, or cookie",
        })?;
        let required = match object.get("required") {
            Some(Value::Bool(value)) => *value,
            Some(_) => {
                return Err(Error::InvalidField {
                    field: format!("{parameter_field}.required"),
                    expected: "a boolean",
                });
            }
            None => location == ParameterLocation::Path,
        };
        if location == ParameterLocation::Path && !required {
            return Err(Error::InvalidField {
                field: format!("{parameter_field}.required"),
                expected: "true for path parameters",
            });
        }
        let style = object
            .get("style")
            .map(|value| string(value, &format!("{parameter_field}.style")))
            .transpose()?
            .unwrap_or_else(|| default_parameter_style(location))
            .to_owned();
        let explode = match object.get("explode") {
            Some(Value::Bool(value)) => *value,
            Some(_) => {
                return Err(Error::InvalidField {
                    field: format!("{parameter_field}.explode"),
                    expected: "a boolean",
                });
            }
            None => style == "form",
        };
        let allow_reserved = match object.get("allowReserved") {
            Some(Value::Bool(value)) => *value,
            Some(_) => {
                return Err(Error::InvalidField {
                    field: format!("{parameter_field}.allowReserved"),
                    expected: "a boolean",
                });
            }
            None => false,
        };
        let schema = parameter_schema(object, &parameter_field)?;
        let identity_name = if location == ParameterLocation::Header {
            name.to_ascii_lowercase()
        } else {
            name.clone()
        };
        parameters.insert(
            (location, identity_name),
            ParameterContract {
                name,
                location,
                required,
                style,
                explode,
                allow_reserved,
                schema,
            },
        );
    }
    Ok(())
}

fn parameter_schema(parameter: &Map<String, Value>, field: &str) -> Result<Option<Value>> {
    if let Some(schema) = parameter.get("schema") {
        return Ok(Some(schema.clone()));
    }
    let Some(content) = parameter.get("content") else {
        return Ok(None);
    };
    let content = object(content, &format!("{field}.content"))?;
    if content.len() != 1 {
        return Err(Error::InvalidField {
            field: format!("{field}.content"),
            expected: "exactly one media type",
        });
    }
    Ok(content
        .values()
        .next()
        .and_then(Value::as_object)
        .and_then(|media| media.get("schema"))
        .cloned())
}

fn parameter_location(value: &str) -> Option<ParameterLocation> {
    match value {
        "path" => Some(ParameterLocation::Path),
        "query" => Some(ParameterLocation::Query),
        "header" => Some(ParameterLocation::Header),
        "cookie" => Some(ParameterLocation::Cookie),
        _ => None,
    }
}

const fn default_parameter_style(location: ParameterLocation) -> &'static str {
    match location {
        ParameterLocation::Path | ParameterLocation::Header => "simple",
        ParameterLocation::Query | ParameterLocation::Cookie => "form",
    }
}

fn response_body(
    document: &Value,
    value: &Value,
    field: &str,
) -> Result<Option<ResponseBodyContract>> {
    let resolved = resolve_local_reference(document, value, field, 0)?;
    let response = object(resolved, field)?;
    let Some(content) = response.get("content") else {
        return Ok(None);
    };
    let content_field = format!("{field}.content");
    let content = media_types(object(content, &content_field)?, &content_field)?;
    Ok((!content.is_empty()).then_some(ResponseBodyContract { content }))
}

fn request_body(document: &Value, value: &Value, field: &str) -> Result<RequestBodyContract> {
    let resolved = resolve_local_reference(document, value, field, 0)?;
    let body = object(resolved, field)?;
    let is_required = match body.get("required") {
        Some(Value::Bool(required)) => *required,
        Some(_) => {
            return Err(Error::InvalidField {
                field: format!("{field}.required"),
                expected: "a boolean",
            });
        }
        None => false,
    };
    let content_field = format!("{field}.content");
    let content = object(required(body, "content")?, &content_field)?;
    if content.is_empty() {
        return Err(Error::InvalidField {
            field: content_field,
            expected: "a non-empty object",
        });
    }
    let content = media_types(content, &content_field)?;
    Ok(RequestBodyContract {
        required: is_required,
        content,
    })
}

fn media_types(
    content: &Map<String, Value>,
    field: &str,
) -> Result<BTreeMap<String, MediaTypeContract>> {
    content
        .iter()
        .map(|(media_type, definition)| {
            let media_field = format!("{field}.{media_type}");
            let definition = object(definition, &media_field)?;
            Ok((
                media_type.to_ascii_lowercase(),
                MediaTypeContract {
                    schema: definition.get("schema").cloned(),
                    example: media_example(definition),
                    examples: media_examples(definition),
                    request_relation: definition.get("x-simile-from-request").cloned(),
                },
            ))
        })
        .collect()
}

fn media_examples(definition: &Map<String, Value>) -> BTreeMap<String, Value> {
    definition
        .get("examples")
        .and_then(Value::as_object)
        .map(|examples| {
            examples
                .iter()
                .filter_map(|(name, example)| {
                    example
                        .get("value")
                        .cloned()
                        .map(|value| (name.clone(), value))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn media_example(definition: &Map<String, Value>) -> Option<Value> {
    definition.get("example").cloned().or_else(|| {
        definition
            .get("examples")
            .and_then(Value::as_object)
            .and_then(|examples| {
                examples
                    .values()
                    .find_map(|example| example.get("value").cloned())
            })
    })
}

fn resolve_local_reference<'a>(
    document: &'a Value,
    value: &'a Value,
    field: &str,
    depth: usize,
) -> Result<&'a Value> {
    if depth >= 64 {
        return Err(Error::ReferenceDepth {
            field: field.to_owned(),
        });
    }
    let Some(reference) = value.get("$ref").and_then(Value::as_str) else {
        return Ok(value);
    };
    let pointer = reference
        .strip_prefix('#')
        .filter(|pointer| pointer.starts_with('/'))
        .ok_or_else(|| Error::UnsupportedReference {
            field: field.to_owned(),
            reference: reference.to_owned(),
        })?;
    let resolved = document
        .pointer(pointer)
        .ok_or_else(|| Error::UnknownReference {
            field: field.to_owned(),
            reference: reference.to_owned(),
        })?;
    resolve_local_reference(document, resolved, field, depth + 1)
}

fn component_schemas(root: &Map<String, Value>) -> Result<BTreeMap<String, Value>> {
    let Some(components) = root.get("components") else {
        return Ok(BTreeMap::new());
    };
    let components = object(components, "components")?;
    let Some(schemas) = components.get("schemas") else {
        return Ok(BTreeMap::new());
    };
    let schemas = object(schemas, "components.schemas")?;
    Ok(schemas
        .iter()
        .map(|(name, schema)| (name.clone(), schema.clone()))
        .collect())
}

fn response_status(value: &str) -> Option<ResponseStatus> {
    if value.eq_ignore_ascii_case("default") {
        return Some(ResponseStatus::Default);
    }
    if value.len() == 3 && value.as_bytes()[1..].eq_ignore_ascii_case(b"xx") {
        let hundreds = value.get(..1)?.parse::<u16>().ok()?;
        return (1..=5)
            .contains(&hundreds)
            .then_some(ResponseStatus::Range(hundreds));
    }
    value
        .parse::<u16>()
        .ok()
        .filter(|status| (100..=599).contains(status))
        .map(ResponseStatus::Exact)
}

fn required<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a Value> {
    object.get(field).ok_or_else(|| Error::InvalidField {
        field: field.to_owned(),
        expected: "present",
    })
}

fn object<'a>(value: &'a Value, field: &str) -> Result<&'a Map<String, Value>> {
    value.as_object().ok_or_else(|| Error::InvalidField {
        field: field.to_owned(),
        expected: "an object",
    })
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value.as_str().ok_or_else(|| Error::InvalidField {
        field: field.to_owned(),
        expected: "a string",
    })
}

fn optional_string(object: &Map<String, Value>, key: &str, field: &str) -> Result<Option<String>> {
    object
        .get(key)
        .map(|value| string(value, field).map(ToOwned::to_owned))
        .transpose()
}

fn string_list(value: &Value, field: &str) -> Result<Vec<String>> {
    let values = value.as_array().ok_or_else(|| Error::InvalidField {
        field: field.to_owned(),
        expected: "an array of strings",
    })?;
    values
        .iter()
        .map(|value| string(value, field).map(ToOwned::to_owned))
        .collect()
}
