#![forbid(unsafe_code)]

mod error;
mod model;
mod node;
mod schema;

use std::collections::{BTreeMap, HashSet};

use serde_json::{Map, Value, json};
use simile_spec::{
    ApiContract, HttpMethod, MediaTypeContract, OpenApiFeature, OpenApiVersion, Operation,
    OperationKey, ParameterContract, ParameterLocation, RequestBodyContract, ResponseBodyContract,
    ResponseContract, ResponseStatus, ServiceInfo,
};
use tree_sitter::{Node, Parser};

pub use error::{Error, Result};

use crate::model::{Model, Shape, ShapeKind, Traits, local_name};

#[derive(Clone, Debug)]
struct LocatedTrait {
    name: &'static str,
    end: usize,
    value: Value,
}

/// Parse a Smithy 2.0 IDL service into Simile's version-neutral HTTP contract.
pub fn parse(source: &str) -> Result<ApiContract> {
    let mut parser = Parser::new();
    parser
        .set_language(tree_sitter_smithy::language())
        .map_err(|_| Error::ParserInitialization)?;
    // OpenAPI extensions use JSON node values that exceed the upstream tree-sitter grammar's
    // practical coverage. Parse those public extensions as JSON and blank only their source span;
    // the actual Smithy shapes remain authoritative and receive the extensions after parsing.
    let (without_extensions, extensions) = extract_openapi_extensions(source)?;
    // `dataExamples` carries arbitrary Smithy node values. Parsing it independently prevents a
    // limitation in the upstream tree-sitter grammar from recovering past, and silently omitting,
    // otherwise valid shapes. Source spans are preserved so examples can be reattached exactly.
    let (without_examples, mut located_traits) =
        extract_located_traits(&without_extensions, "dataExamples", "dataExamples trait")?;
    // Some valid regular-expression characters are rejected by the old tree-sitter Smithy
    // grammar. Preserve the standard trait independently instead of weakening its validation.
    let (without_patterns, patterns) =
        extract_located_traits(&without_examples, "pattern", "pattern trait")?;
    located_traits.extend(patterns);
    // Smithy permits optional commas in node arrays and trait structures. The upstream Smithy 2
    // tree-sitter grammar accepts them in objects but not arrays, so normalize separator commas to
    // spaces while preserving every byte offset used for diagnostics and source extraction.
    let normalized = normalize_separator_commas(&without_patterns);
    // The grammar also mistakes `//` inside quoted strings for a line comment. Mask only the
    // parser's same-length view; model values are sliced from the unmodified semantic source.
    let parser_source = mask_comment_markers_in_strings(&normalized);
    let tree = parser.parse(&parser_source, None).ok_or(Error::Parse)?;
    let root = tree.root_node();
    let mut model = Model::parse(root, without_patterns.as_bytes())?;
    if root.has_error() {
        let error = first_error(root).unwrap_or(root);
        let position = error.start_position();
        return Err(Error::Syntax {
            line: position.row + 1,
            column: position.column + 1,
        });
    }
    model.apply_located_traits(located_traits)?;
    model.apply_openapi_extensions(extensions)?;
    contract(&model)
}

fn mask_comment_markers_in_strings(source: &str) -> String {
    let mut bytes = source.as_bytes().to_vec();
    let mut index = 0;
    let mut quoted = false;
    let mut text_block = false;
    while index < bytes.len() {
        if index + 2 < bytes.len() && &bytes[index..index + 3] == b"\"\"\"" {
            text_block = !text_block;
            index += 3;
            continue;
        }
        match bytes[index] {
            b'\\' if quoted || text_block => index += 2,
            b'"' if !text_block => {
                quoted = !quoted;
                index += 1;
            }
            b'/' if (quoted || text_block)
                && index + 1 < bytes.len()
                && bytes[index + 1] == b'/' =>
            {
                bytes[index] = b'|';
                bytes[index + 1] = b'|';
                index += 2;
            }
            _ => index += 1,
        }
    }
    // ASCII substitutions preserve UTF-8 validity and all byte offsets.
    String::from_utf8(bytes).unwrap_or_else(|_| source.to_owned())
}

fn extract_located_traits(
    source: &str,
    name: &'static str,
    context: &'static str,
) -> Result<(String, Vec<LocatedTrait>)> {
    let prefix = format!("@{name}(");
    let mut bytes = source.as_bytes().to_vec();
    let mut traits = Vec::new();
    let mut cursor = 0;
    while let Some(relative_start) = source[cursor..].find(&prefix) {
        let start = cursor + relative_start;
        let content_start = start + prefix.len();
        let relative_end =
            balanced_parenthesis_end(&source[content_start..]).ok_or_else(|| Error::Invalid {
                context,
                detail: "unclosed trait value".to_owned(),
            })?;
        let end = content_start + relative_end;
        traits.push(LocatedTrait {
            name,
            end,
            value: node::parse(&source[content_start..end])?,
        });
        blank_preserving_lines(&mut bytes[start..=end]);
        cursor = end + 1;
    }
    String::from_utf8(bytes)
        .map(|source| (source, traits))
        .map_err(|error| Error::Invalid {
            context: "source",
            detail: error.to_string(),
        })
}

fn extract_openapi_extensions(source: &str) -> Result<(String, BTreeMap<String, Value>)> {
    const PREFIX: &str = "@openapiExtensions(";
    let mut bytes = source.as_bytes().to_vec();
    let mut extensions = BTreeMap::new();
    let mut cursor = 0;
    while let Some(relative_start) = source[cursor..].find(PREFIX) {
        let start = cursor + relative_start;
        let content_start = start + PREFIX.len();
        let relative_end =
            balanced_parenthesis_end(&source[content_start..]).ok_or_else(|| Error::Invalid {
                context: "openapiExtensions trait",
                detail: "unclosed trait value".to_owned(),
            })?;
        let end = content_start + relative_end;
        let wrapped = format!("{{{}}}", &source[content_start..end]);
        let mut value = node::parse(&wrapped)?;
        let object = value.as_object_mut().ok_or_else(|| Error::Invalid {
            context: "openapiExtensions trait",
            detail: "trait value must be an object".to_owned(),
        })?;
        let shape_name = next_shape_name(&source[end + 1..]).ok_or_else(|| Error::Invalid {
            context: "openapiExtensions trait",
            detail: "trait is not followed by a shape".to_owned(),
        })?;
        extensions.insert(shape_name, Value::Object(object.clone()));
        blank_preserving_lines(&mut bytes[start..=end]);
        cursor = end + 1;
    }
    String::from_utf8(bytes)
        .map(|source| (source, extensions))
        .map_err(|error| Error::Invalid {
            context: "source",
            detail: error.to_string(),
        })
}

fn blank_preserving_lines(bytes: &mut [u8]) {
    for byte in bytes {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}

fn balanced_parenthesis_end(source: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 1_u32;
    let mut quoted = false;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if quoted => index += 2,
            b'"' => {
                quoted = !quoted;
                index += 1;
            }
            b'(' if !quoted => {
                depth += 1;
                index += 1;
            }
            b')' if !quoted => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    None
}

fn next_shape_name(source: &str) -> Option<String> {
    const SHAPE_KEYWORDS: &[&str] = &[
        "service",
        "operation",
        "structure",
        "union",
        "list",
        "map",
        "enum",
        "intEnum",
        "blob",
        "boolean",
        "string",
        "byte",
        "short",
        "integer",
        "long",
        "float",
        "double",
        "bigInteger",
        "bigDecimal",
        "timestamp",
        "document",
    ];
    for line in source.lines() {
        let line = line.trim_start();
        for keyword in SHAPE_KEYWORDS {
            if let Some(rest) = line
                .strip_prefix(keyword)
                .and_then(|rest| rest.strip_prefix(' '))
            {
                let name = rest
                    .split(|character: char| character.is_whitespace() || character == '{')
                    .next()?;
                if !name.is_empty() {
                    return Some(name.to_owned());
                }
            }
        }
    }
    None
}

fn normalize_separator_commas(source: &str) -> String {
    let mut bytes = source.as_bytes().to_vec();
    let mut index = 0;
    let mut quoted = false;
    let mut text_block = false;
    while index < bytes.len() {
        if index + 2 < bytes.len() && &bytes[index..index + 3] == b"\"\"\"" {
            text_block = !text_block;
            index += 3;
            continue;
        }
        if text_block {
            index += 1;
            continue;
        }
        if !quoted && index + 1 < bytes.len() && &bytes[index..index + 2] == b"//" {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        match bytes[index] {
            b'\\' if quoted => index += 2,
            b'\n' => {
                quoted = false;
                index += 1;
            }
            b'"' => {
                quoted = !quoted;
                index += 1;
            }
            b',' if !quoted => {
                bytes[index] = b' ';
                index += 1;
            }
            _ => index += 1,
        }
    }
    // Replacing ASCII commas with spaces preserves the original UTF-8 validity.
    String::from_utf8(bytes).unwrap_or_else(|_| source.to_owned())
}

fn contract(model: &Model) -> Result<ApiContract> {
    let service = model.service()?;
    let ShapeKind::Service {
        version,
        operations: service_operations,
    } = &service.kind
    else {
        return Err(Error::MissingService);
    };
    let operation_names = if service_operations.is_empty() {
        model
            .shapes
            .values()
            .filter(|shape| matches!(shape.kind, ShapeKind::Operation { .. }))
            .map(|shape| shape.name.clone())
            .collect::<Vec<_>>()
    } else {
        service_operations.clone()
    };
    let mut operations = BTreeMap::new();
    let mut operation_ids = HashSet::new();
    for operation_name in operation_names {
        let shape = model
            .shapes
            .get(local_name(&operation_name))
            .ok_or_else(|| Error::UnknownOperation(operation_name.clone()))?;
        if !operation_ids.insert(shape.name.clone()) {
            return Err(Error::DuplicateOperationId {
                operation_id: shape.name.clone(),
            });
        }
        let operation = operation(model, shape)?;
        operations.insert(operation.key.clone(), operation);
    }

    Ok(ApiContract {
        // Native Smithy shapes lower to schemas accepted by both dialects. A Smithy projection can
        // also retain exact OpenAPI 3.0 fragments for composition rules with no Smithy equivalent,
        // so the compiler must preserve 3.0 nullability and exclusive-bound semantics.
        openapi: OpenApiVersion {
            exact: "3.0.3-compatible Smithy 2.0".to_owned(),
            feature: OpenApiFeature::V3_0,
        },
        info: ServiceInfo {
            title: service.name.clone(),
            version: version.clone(),
            description: documentation(&service.traits),
        },
        operations,
        schemas: schema::components(model)?,
    })
}

fn operation(model: &Model, shape: &Shape) -> Result<Operation> {
    let ShapeKind::Operation {
        input,
        output,
        errors,
    } = &shape.kind
    else {
        return Err(Error::UnknownOperation(shape.name.clone()));
    };
    let http = shape.traits.get("http").and_then(Value::as_object).ok_or(
        Error::MissingOperationTrait {
            operation: shape.name.clone(),
            trait_name: "http",
        },
    )?;
    let method = http
        .get("method")
        .and_then(Value::as_str)
        .map(http_method)
        .ok_or_else(|| Error::Invalid {
            context: "http trait",
            detail: format!("operation `{}` has no method", shape.name),
        })?;
    let path = http
        .get("uri")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Invalid {
            context: "http trait",
            detail: format!("operation `{}` has no URI", shape.name),
        })?
        .to_owned();
    let success_status = http.get("code").and_then(Value::as_i64).unwrap_or(200);
    let success_status = status(&shape.name, success_status)?;
    let relation_extensions = response_relations(&shape.traits);

    let request = input
        .as_deref()
        .map(|name| payload(model, name))
        .transpose()?
        .flatten();
    let raw_request_body = request.as_ref().map(|body| {
        json!({
            "required": body.required,
            "content": body.content.iter().map(|(kind, media)| {
                (kind.clone(), json!({ "schema": media.schema }))
            }).collect::<Map<String, Value>>()
        })
    });
    let parameters = input
        .as_deref()
        .map(|input| operation_parameters(model, input))
        .transpose()?
        .unwrap_or_default();

    let mut responses = Vec::new();
    responses.push(response(
        model,
        output.as_deref(),
        success_status,
        relation_extensions.as_ref(),
    )?);
    for error in errors {
        let error_shape = model.shape(error)?;
        let status_value = error_shape
            .traits
            .get("httpError")
            .and_then(Value::as_i64)
            .ok_or(Error::MissingOperationTrait {
                operation: shape.name.clone(),
                trait_name: "httpError",
            })?;
        responses.push(response(
            model,
            Some(error),
            status(&shape.name, status_value)?,
            relation_extensions.as_ref(),
        )?);
    }
    responses.sort_by(|left, right| left.status.cmp(&right.status));

    let key = OperationKey::new(&method, &path);
    Ok(Operation {
        key,
        operation_id: Some(shape.name.clone()),
        method,
        path,
        summary: documentation(&shape.traits),
        tags: shape
            .traits
            .get("tags")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        parameters,
        request_body: raw_request_body,
        request,
        responses,
    })
}

fn operation_parameters(model: &Model, input: &str) -> Result<Vec<ParameterContract>> {
    if local_name(input) == "Unit" {
        return Ok(Vec::new());
    }
    let shape = model.shape(input)?;
    let ShapeKind::Structure { members, .. } = &shape.kind else {
        return Ok(Vec::new());
    };
    members
        .iter()
        .filter_map(|member| {
            let binding = if member.traits.contains("httpLabel") {
                Some((
                    member.name.as_str(),
                    ParameterLocation::Path,
                    "simple",
                    false,
                ))
            } else if let Some(name) = member.traits.get("httpQuery").and_then(Value::as_str) {
                Some((name, ParameterLocation::Query, "form", true))
            } else if let Some(name) = member.traits.get("httpHeader").and_then(Value::as_str) {
                Some((name, ParameterLocation::Header, "simple", false))
            } else {
                member
                    .traits
                    .get("httpCookie")
                    .and_then(Value::as_str)
                    .map(|name| (name, ParameterLocation::Cookie, "form", true))
            }?;
            Some(
                schema::member_schema(model, member).map(|schema| ParameterContract {
                    name: binding.0.to_owned(),
                    location: binding.1,
                    // Why: Smithy path labels are intrinsically required; other bindings use the
                    // member's explicit required trait.
                    required: binding.1 == ParameterLocation::Path
                        || member.traits.contains("required"),
                    style: binding.2.to_owned(),
                    explode: binding.3,
                    allow_reserved: false,
                    schema: Some(schema),
                }),
            )
        })
        .collect()
}

fn response(
    model: &Model,
    output: Option<&str>,
    status: u16,
    relations: Option<&Map<String, Value>>,
) -> Result<ResponseContract> {
    let mut body = output
        .map(|name| payload(model, name))
        .transpose()?
        .flatten()
        .map(|request| ResponseBodyContract {
            content: request.content,
        });
    if let Some(body) = &mut body {
        let status_key = status.to_string();
        if let Some(status_relations) = relations
            .and_then(|values| values.get(&status_key))
            .and_then(Value::as_object)
        {
            for (content_type, relation) in status_relations {
                if let Some(media) = body.content.get_mut(&content_type.to_ascii_lowercase()) {
                    media.request_relation = Some(relation.clone());
                }
            }
        }
    }
    let definition = match &body {
        Some(body) => json!({
            "description": format!("HTTP {status} response"),
            "content": body.content.iter().map(|(kind, media)| {
                (kind.clone(), json!({ "schema": media.schema }))
            }).collect::<Map<String, Value>>()
        }),
        None => json!({ "description": format!("HTTP {status} response") }),
    };
    Ok(ResponseContract {
        status: ResponseStatus::Exact(status),
        definition,
        body,
    })
}

fn payload(model: &Model, name: &str) -> Result<Option<RequestBodyContract>> {
    if local_name(name) == "Unit" {
        return Ok(None);
    }
    let shape = model.shape(name)?;
    let ShapeKind::Structure { members, .. } = &shape.kind else {
        return Ok(None);
    };
    let Some(member) = members
        .iter()
        .find(|member| member.traits.contains("httpPayload"))
    else {
        return Ok(None);
    };
    let content_type = member
        .traits
        .get("contentType")
        .and_then(Value::as_str)
        .unwrap_or("application/json")
        .to_ascii_lowercase();
    let values = schema::examples(&member.traits);
    let examples = values
        .into_iter()
        .enumerate()
        .map(|(index, value)| (format!("example_{}", index + 1), value))
        .collect();
    Ok(Some(RequestBodyContract {
        required: member.traits.contains("required"),
        content: BTreeMap::from([(
            content_type,
            MediaTypeContract {
                schema: Some(schema::member_schema(model, member)?),
                example: None,
                examples,
                request_relation: None,
            },
        )]),
    }))
}

fn response_relations(traits: &Traits) -> Option<Map<String, Value>> {
    traits
        .get("openapiExtensions")
        .and_then(Value::as_object)
        .and_then(|extensions| extensions.get("x-inttegro-simile-response-relations"))
        .and_then(Value::as_object)
        .cloned()
}

fn documentation(traits: &Traits) -> Option<String> {
    traits
        .get("documentation")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn http_method(value: &str) -> HttpMethod {
    match value.to_ascii_uppercase().as_str() {
        "GET" => HttpMethod::Get,
        "PUT" => HttpMethod::Put,
        "POST" => HttpMethod::Post,
        "DELETE" => HttpMethod::Delete,
        "OPTIONS" => HttpMethod::Options,
        "HEAD" => HttpMethod::Head,
        "PATCH" => HttpMethod::Patch,
        "TRACE" => HttpMethod::Trace,
        "QUERY" => HttpMethod::Query,
        other => HttpMethod::Additional(other.to_owned()),
    }
}

fn status(operation: &str, value: i64) -> Result<u16> {
    u16::try_from(value)
        .ok()
        .filter(|status| (100..=599).contains(status))
        .ok_or_else(|| Error::InvalidStatus {
            operation: operation.to_owned(),
            status: value,
        })
}

fn first_error(node: Node<'_>) -> Option<Node<'_>> {
    if node.is_error() || node.is_missing() {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.children(&mut cursor).find_map(first_error)
}

#[cfg(test)]
mod tests {
    #[test]
    fn parser_accepts_pattern_trait_after_blank_lines() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(tree_sitter_smithy::language())
            .unwrap_or_else(|error| panic!("Smithy language must load: {error}"));
        let source = "$version: \"2\"\nnamespace test\nstructure PurchaseIntent {\n\n\n    @required\n    allow_variants: Boolean\n\n\n    @pattern(\"^app_\")\n    @required\n    application_id: String\n}\n";
        let tree = parser
            .parse(source, None)
            .unwrap_or_else(|| panic!("Smithy parser must return a tree"));
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );
    }

    #[test]
    fn parser_accepts_url_enum_value() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(tree_sitter_smithy::language())
            .unwrap_or_else(|error| panic!("Smithy language must load: {error}"));
        let source = "$version: \"2\"\nnamespace test\nenum Url {\n  studio = \"https://studio.inttegro.com/errors\"\n}\n";
        let parser_source = mask_comment_markers_in_strings(source);
        let tree = parser
            .parse(&parser_source, None)
            .unwrap_or_else(|| panic!("Smithy parser must return a tree"));
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );
    }

    #[test]
    fn discriminator_members_are_materialized_on_closed_target_shapes() {
        let contract = parse(
            r##"$version: "2.0"
namespace example.animals

structure Cat { @required name: String }
structure Dog { @required name: String }

@untagged
@openapiExtensions(
  "x-inttegro-openapi-source-schema": {
    "discriminator": {
      "propertyName": "type"
      "mapping": {
        "cat": "#/components/schemas/Cat"
        "dog": "#/components/schemas/Dog"
      }
    }
    "oneOf": [
      { "$ref": "#/components/schemas/Cat" }
      { "$ref": "#/components/schemas/Dog" }
    ]
  }
)
union Animal { Cat: Cat, Dog: Dog }

structure GetAnimalInput {}
structure GetAnimalOutput {
  @httpPayload @required @contentType("application/json") body: Animal
}
@http(method: "GET", uri: "/animal", code: 200)
operation getAnimal { input: GetAnimalInput, output: GetAnimalOutput }
service Animals { version: "1.0.0", operations: [getAnimal] }
"##,
        )
        .unwrap_or_else(|error| panic!("discriminator contract must parse: {error}"));

        assert_eq!(
            contract.schemas["Cat"]["properties"]["type"]["enum"][0],
            "cat"
        );
        assert_eq!(
            contract.schemas["Dog"]["properties"]["type"]["enum"][0],
            "dog"
        );
        assert!(
            contract.schemas["Cat"]["required"]
                .as_array()
                .is_some_and(|required| required.iter().any(|value| value == "type"))
        );
        assert_eq!(
            contract.schemas["Animal"]["oneOf"][0]["$ref"],
            "#/components/schemas/Cat"
        );
    }

    use super::*;

    #[test]
    fn parses_a_smithy_http_service() {
        let contract = parse(
            r#"$version: "2.0"
namespace example.weather

@openapiExtensions(
    "x-simile-constraints": [
        {
            "ordered": {
                "paths": [
                    "/created_at"
                    "/updated_at"
                ]
            }
        }
    ]
)
structure Weather {
    @dataExamples([{ json: "public, max-age=31536000, immutable" }])
    @required
    city: String
    @required
    created_at: Timestamp
    @required
    updated_at: Timestamp
}

structure GetWeatherInput {}

structure GetWeatherOutput {
    @httpPayload
    @required
    @contentType("application/json")
    body: Weather
}

@error("client")
@httpError(404)
structure NotFound {
    @httpPayload
    @required
    @contentType("application/json")
    body: Weather
}

@http(method: "GET", uri: "/weather", code: 200)
@tags(["Weather"])
operation getWeather {
    input: GetWeatherInput
    output: GetWeatherOutput
    errors: [NotFound]
}

service WeatherService {
    version: "1.0.0"
    operations: [getWeather]
}
"#,
        )
        .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(contract.info.title, "WeatherService");
        assert_eq!(contract.operations.len(), 1);
        let operation = contract
            .operation_by_id("getWeather")
            .unwrap_or_else(|| panic!("operation missing"));
        assert_eq!(operation.path, "/weather");
        assert_eq!(operation.responses.len(), 2);
        assert!(
            contract.schemas["Weather"]
                .get("x-simile-constraints")
                .is_some()
        );
    }

    #[test]
    fn lowers_smithy_http_binding_traits_to_parameters() {
        let contract = parse(
            r#"$version: "2.0"
namespace example.orders

structure GetOrderInput {
    @httpLabel
    @required
    order_id: String
    @httpQuery("expand")
    expand: String
    @httpHeader("X-Request-Id")
    request_id: String
}

structure GetOrderOutput {}

@http(method: "GET", uri: "/orders/{order_id}", code: 204)
operation getOrder {
    input: GetOrderInput
    output: GetOrderOutput
}

service Orders { version: "1.0.0", operations: [getOrder] }
"#,
        )
        .unwrap_or_else(|error| panic!("binding contract must parse: {error}"));

        let operation = contract
            .operation_by_id("getOrder")
            .unwrap_or_else(|| panic!("operation must exist"));
        assert_eq!(operation.parameters.len(), 3);
        assert_eq!(operation.parameters[0].name, "order_id");
        assert_eq!(operation.parameters[0].location, ParameterLocation::Path);
        assert!(operation.parameters[0].required);
        assert_eq!(operation.parameters[1].name, "expand");
        assert_eq!(operation.parameters[1].location, ParameterLocation::Query);
        assert!(!operation.parameters[1].required);
        assert_eq!(operation.parameters[2].name, "X-Request-Id");
    }
}
