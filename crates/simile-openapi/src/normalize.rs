use std::collections::{BTreeMap, HashMap};

use serde_json::{Map, Value};
use simile_spec::{
    ApiContract, HttpMethod, MediaTypeContract, OpenApiVersion, Operation, OperationKey,
    RequestBodyContract, ResponseContract, ResponseStatus, ServiceInfo,
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
            let operation = operation(document, path, method_name, method, value)?;
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
    method_name: &str,
    method: HttpMethod,
    value: &Value,
) -> Result<Operation> {
    let field = format!("paths.{path}.{method_name}");
    let body = object(value, &field)?;
    let operation_id = optional_string(body, "operationId", "operationId")?;
    let summary = optional_string(body, "summary", "summary")?;
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
            Ok(ResponseContract {
                status: response_status(status).ok_or_else(|| Error::InvalidResponseStatus {
                    method: method.to_string(),
                    path: path.to_owned(),
                    status: status.clone(),
                })?,
                definition: definition.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let raw_request_body = body.get("requestBody").cloned();
    let request = body
        .get("requestBody")
        .map(|value| request_body(document, value, &format!("{field}.requestBody")))
        .transpose()?;

    Ok(Operation {
        key: OperationKey::new(&method, path),
        operation_id,
        method,
        path: path.to_owned(),
        summary,
        request_body: raw_request_body,
        request,
        responses,
    })
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
    let content = content
        .iter()
        .map(|(media_type, definition)| {
            let media_field = format!("{field}.content.{media_type}");
            let definition = object(definition, &media_field)?;
            Ok((
                media_type.to_ascii_lowercase(),
                MediaTypeContract {
                    schema: definition.get("schema").cloned(),
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    Ok(RequestBodyContract {
        required: is_required,
        content,
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
