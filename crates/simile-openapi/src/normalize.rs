use std::collections::{BTreeMap, HashMap};

use serde_json::{Map, Value};
use simile_spec::{
    ApiContract, HttpMethod, MediaTypeContract, OpenApiVersion, Operation, OperationKey,
    ParameterContract, ParameterLocation, RequestBodyContract, ResponseBodyContract,
    ResponseContract, ResponseStatus, SecurityRequirement, SecurityScheme, ServiceInfo,
    WebhookContract, WebhookOrigin,
};

use crate::{Error, Result};

pub(crate) fn contract(
    document: &Value,
    version: OpenApiVersion,
    supports_extended_methods: bool,
    supports_top_level_webhooks: bool,
) -> Result<ApiContract> {
    let root = object(document, "document")?;
    let info = object(required(root, "info")?, "info")?;
    let info = ServiceInfo {
        title: string(required(info, "title")?, "info.title")?.to_owned(),
        version: string(required(info, "version")?, "info.version")?.to_owned(),
        description: optional_string(info, "description", "info.description")?,
    };
    let paths = object(required(root, "paths")?, "paths")?;
    let security_schemes = security_schemes(root)?;
    let global_security = security_requirements(root.get("security"), "security")?;
    let mut operations = BTreeMap::new();
    let mut webhooks = BTreeMap::new();
    let mut operation_ids: HashMap<String, OperationKey> = HashMap::new();

    for (path, path_value) in paths {
        let path_item = object(path_value, &format!("paths.{path}"))?;
        let mut entries =
            operation_entries(path_item, supports_extended_methods).collect::<Vec<_>>();
        if supports_extended_methods {
            entries.extend(additional_operation_entries(path_item)?);
        }
        for (method_name, method, value) in entries {
            let operation = operation(
                document,
                path,
                path_item,
                method_name,
                method,
                value,
                &global_security,
            )?;
            collect_callback_webhooks(
                document,
                &operation,
                value,
                supports_extended_methods,
                &mut webhooks,
            )?;
            register_operation_id(&mut operation_ids, &operation)?;
            operations.insert(operation.key.clone(), operation);
        }
    }

    if supports_top_level_webhooks {
        collect_top_level_webhooks(document, root, supports_extended_methods, &mut webhooks)?;
    }

    Ok(ApiContract {
        openapi: version,
        info,
        operations,
        security_schemes,
        webhooks,
        schemas: component_schemas(root)?,
    })
}

fn collect_top_level_webhooks(
    document: &Value,
    root: &Map<String, Value>,
    supports_extended_methods: bool,
    webhooks: &mut BTreeMap<String, WebhookContract>,
) -> Result<()> {
    let Some(value) = root.get("webhooks") else {
        return Ok(());
    };
    let entries = object(value, "webhooks")?;
    for (name, path_item) in entries {
        collect_webhook_path_item(
            document,
            name,
            None,
            path_item,
            WebhookOrigin::OpenApiWebhook,
            supports_extended_methods,
            webhooks,
        )?;
    }
    Ok(())
}

fn collect_callback_webhooks(
    document: &Value,
    operation: &Operation,
    raw_operation: &Value,
    supports_extended_methods: bool,
    webhooks: &mut BTreeMap<String, WebhookContract>,
) -> Result<()> {
    let operation_body = object(raw_operation, operation.key.as_str())?;
    let Some(callbacks) = operation_body.get("callbacks") else {
        return Ok(());
    };
    let callbacks = resolve_local_reference(document, callbacks, "callbacks", 0)?;
    for (callback_name, callback) in object(callbacks, "callbacks")? {
        let callback =
            resolve_local_reference(document, callback, &format!("callbacks.{callback_name}"), 0)?;
        for (target, path_item) in object(callback, &format!("callbacks.{callback_name}"))? {
            let owner = operation
                .operation_id
                .as_deref()
                .unwrap_or_else(|| operation.key.as_str());
            collect_webhook_path_item(
                document,
                &format!("{owner}.{callback_name}"),
                Some(target.clone()),
                path_item,
                WebhookOrigin::OpenApiCallback,
                supports_extended_methods,
                webhooks,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn collect_webhook_path_item(
    document: &Value,
    base_name: &str,
    target: Option<String>,
    path_item: &Value,
    origin: WebhookOrigin,
    supports_extended_methods: bool,
    webhooks: &mut BTreeMap<String, WebhookContract>,
) -> Result<()> {
    let path_item = resolve_local_reference(document, path_item, base_name, 0)?;
    let path_item = object(path_item, base_name)?;
    let mut entries = operation_entries(path_item, supports_extended_methods).collect::<Vec<_>>();
    if supports_extended_methods {
        entries.extend(additional_operation_entries(path_item)?);
    }
    let multiple = entries.len() > 1;
    for (method_name, method, value) in entries {
        let field = format!("webhook.{base_name}.{method_name}");
        let body = object(value, &field)?;
        let operation_id = optional_string(body, "operationId", &format!("{field}.operationId"))?;
        let name = operation_id.clone().unwrap_or_else(|| {
            if multiple {
                format!("{base_name}.{}", method.as_str().to_ascii_lowercase())
            } else {
                base_name.to_owned()
            }
        });
        let request = body
            .get("requestBody")
            .map(|value| request_body(document, value, &format!("{field}.requestBody")))
            .transpose()?;
        let contract = WebhookContract {
            name: name.clone(),
            method,
            operation_id,
            summary: optional_string(body, "summary", &format!("{field}.summary"))?,
            target: target.clone(),
            request,
            origin,
        };
        if webhooks.insert(name.clone(), contract).is_some() {
            return Err(Error::DuplicateWebhook { name });
        }
    }
    Ok(())
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
    global_security: &[SecurityRequirement],
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
    let security = if body.contains_key("security") {
        security_requirements(body.get("security"), &format!("{field}.security"))?
    } else {
        global_security.to_vec()
    };

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
        security,
    })
}

fn security_schemes(root: &Map<String, Value>) -> Result<BTreeMap<String, SecurityScheme>> {
    let Some(components) = root.get("components") else {
        return Ok(BTreeMap::new());
    };
    let components = object(components, "components")?;
    let Some(schemes) = components.get("securitySchemes") else {
        return Ok(BTreeMap::new());
    };
    object(schemes, "components.securitySchemes")?
        .iter()
        .map(|(name, raw)| {
            let field = format!("components.securitySchemes.{name}");
            let scheme = object(raw, &field)?;
            let kind = string(required(scheme, "type")?, &format!("{field}.type"))?;
            let scheme = match kind {
                "apiKey" => SecurityScheme::ApiKey {
                    location: parameter_location(string(
                        required(scheme, "in")?,
                        &format!("{field}.in"),
                    )?)
                    .filter(|location| *location != ParameterLocation::Path)
                    .ok_or_else(|| Error::InvalidField {
                        field: format!("{field}.in"),
                        expected: "one of query, header, or cookie",
                    })?,
                    name: string(required(scheme, "name")?, &format!("{field}.name"))?.to_owned(),
                },
                "http" => SecurityScheme::Http {
                    scheme: string(required(scheme, "scheme")?, &format!("{field}.scheme"))?
                        .to_ascii_lowercase(),
                    bearer_format: optional_string(
                        scheme,
                        "bearerFormat",
                        &format!("{field}.bearerFormat"),
                    )?,
                },
                "oauth2" => SecurityScheme::OAuth2,
                "openIdConnect" => SecurityScheme::OpenIdConnect,
                "mutualTLS" => SecurityScheme::MutualTls,
                _ => {
                    return Err(Error::InvalidField {
                        field: format!("{field}.type"),
                        expected: "apiKey, http, oauth2, openIdConnect, or mutualTLS",
                    });
                }
            };
            Ok((name.clone(), scheme))
        })
        .collect()
}

fn security_requirements(raw: Option<&Value>, field: &str) -> Result<Vec<SecurityRequirement>> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let alternatives = raw.as_array().ok_or_else(|| Error::InvalidField {
        field: field.to_owned(),
        expected: "an array",
    })?;
    alternatives
        .iter()
        .enumerate()
        .map(|(index, alternative)| {
            let schemes = object(alternative, &format!("{field}.{index}"))?
                .iter()
                .map(|(name, scopes)| {
                    Ok((
                        name.clone(),
                        string_list(scopes, &format!("{field}.{index}.{name}"))?,
                    ))
                })
                .collect::<Result<BTreeMap<_, _>>>()?;
            Ok(SecurityRequirement { schemes })
        })
        .collect()
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
