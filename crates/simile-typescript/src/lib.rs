#![forbid(unsafe_code)]

//! Semantic TypeScript declaration frontend for Simile.

mod ast;
mod constraints;
mod error;
mod schema;

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde_json::{Map, Value, json};
use simile_spec::{
    ApiContract, HttpMethod, MediaTypeContract, Operation, OperationKey, ParameterContract,
    ParameterLocation, RequestBodyContract, ResponseBodyContract, ResponseContract, ResponseStatus,
    SchemaFeature, SchemaVersion, ServiceInfo,
};

use crate::ast::{ContractAst, TypeExpr};

pub use error::{Error, Result};

/// One declaration file in a semantic TypeScript contract package.
#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    pub name: &'a str,
    pub contents: &'a str,
}

impl<'a> Source<'a> {
    #[must_use]
    pub const fn new(name: &'a str, contents: &'a str) -> Self {
        Self { name, contents }
    }
}

/// Compile declarations-only TypeScript contract types into Simile's canonical HTTP contract.
///
/// The frontend parses declarations and literal types. It does not execute JavaScript, invoke the
/// TypeScript compiler, or depend on a source-format intermediate representation.
pub fn parse(sources: &[Source<'_>]) -> Result<ApiContract> {
    let ast = ContractAst::parse(sources)?;
    contract(&ast, sources)
}

/// Return module specifiers referenced by one declaration file.
///
/// Simile Core uses this declarations-only view to load a split contract package without invoking
/// the TypeScript compiler or executing JavaScript.
pub fn module_specifiers(source: Source<'_>) -> Result<Vec<String>> {
    ast::module_specifiers(&source)
}

fn contract(ast: &ContractAst, sources: &[Source<'_>]) -> Result<ApiContract> {
    let mut operations = BTreeMap::new();
    let mut operation_ids = HashSet::new();
    for (group_name, group) in ast
        .declarations
        .iter()
        .filter(|(name, _)| name.ends_with("Operations") && name.as_str() != "Operations")
    {
        let Some(definitions) = group.properties() else {
            continue;
        };
        let tag = group_name.trim_end_matches("Operations");
        for definition in definitions {
            if !operation_ids.insert(definition.name.clone()) {
                return Err(Error::DuplicateOperation(definition.name.clone()));
            }
            let operation = operation(ast, &definition.name, tag, &definition.value)?;
            if operations
                .insert(operation.key.clone(), operation)
                .is_some()
            {
                return Err(Error::InvalidOperation {
                    operation: definition.name.clone(),
                    detail: "method and path duplicate another operation".to_owned(),
                });
            }
        }
    }
    if operations.is_empty() {
        return Err(Error::Missing("an *Operations interface"));
    }

    let title = ast
        .declarations
        .get("ServiceName")
        .and_then(TypeExpr::literal_string)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            sources
                .first()
                .and_then(|source| Path::new(source.name).file_stem())
                .and_then(|name| name.to_str())
                .unwrap_or("TypeScript service")
                .to_owned()
        });
    let version = ast
        .declarations
        .get("CollectionVersion")
        .and_then(TypeExpr::literal_string)
        .unwrap_or("0.0.0")
        .to_owned();

    Ok(ApiContract::from_schema(
        // Why: the TypeScript package is an independent contract source, so its canonical schema
        // capability is stated directly instead of borrowing another frontend's source version.
        SchemaVersion {
            exact: "JSON Schema 2020-12-compatible TypeScript declarations".to_owned(),
            feature: SchemaFeature::V3_1,
        },
        ServiceInfo {
            title,
            version,
            description: Some("Semantic TypeScript declaration contract".to_owned()),
        },
        operations,
        schema::components(ast)?,
    ))
}

fn operation(
    ast: &ContractAst,
    operation_id: &str,
    tag: &str,
    definition: &TypeExpr,
) -> Result<Operation> {
    let method = required_literal(definition, "method", operation_id)?;
    let method = http_method(method);
    let path = required_literal(definition, "path", operation_id)?.to_owned();
    let request = definition
        .property("request")
        .filter(|request| !matches!(request, TypeExpr::Null))
        .map(|request| request_contract(ast, operation_id, request))
        .transpose()?;
    let request_body = request.as_ref().map(|request| {
        json!({
            "required": request.required,
            "content": request.content.iter().map(|(kind, media)| {
                (kind.clone(), json!({ "schema": media.schema }))
            }).collect::<Map<String, Value>>()
        })
    });
    let parameters = operation_parameters(ast, operation_id, definition)?;
    let responses = response_contracts(ast, operation_id, definition)?;
    let tags = definition
        .property("tags")
        .and_then(|value| match value {
            TypeExpr::Tuple(values) => Some(
                values
                    .iter()
                    .filter_map(TypeExpr::literal_string)
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| vec![tag.to_owned()]);
    let key = OperationKey::new(&method, &path);
    Ok(Operation {
        key,
        operation_id: Some(operation_id.to_owned()),
        method,
        path,
        summary: None,
        tags,
        parameters,
        request_body,
        request,
        responses,
    })
}

fn operation_parameters(
    ast: &ContractAst,
    operation_id: &str,
    definition: &TypeExpr,
) -> Result<Vec<ParameterContract>> {
    let Some(parameters) = definition.property("parameters") else {
        return Ok(Vec::new());
    };
    let Some(locations) = parameters.properties() else {
        return Err(Error::InvalidOperation {
            operation: operation_id.to_owned(),
            detail: "parameters must be an object grouped by HTTP location".to_owned(),
        });
    };
    let mut result = Vec::new();
    for location in locations {
        let (location_kind, style, explode) = match location.name.as_str() {
            "path" => (ParameterLocation::Path, "simple", false),
            "query" => (ParameterLocation::Query, "form", true),
            "header" => (ParameterLocation::Header, "simple", false),
            "cookie" => (ParameterLocation::Cookie, "form", true),
            other => {
                return Err(Error::InvalidOperation {
                    operation: operation_id.to_owned(),
                    detail: format!("parameters.{other} is not a supported HTTP location"),
                });
            }
        };
        let values = match &location.value {
            TypeExpr::Object { properties, .. } => properties.as_slice(),
            TypeExpr::Reference { name, .. } if name == "Record" => &[],
            _ => {
                return Err(Error::InvalidOperation {
                    operation: operation_id.to_owned(),
                    detail: format!("parameters.{} must be an object", location.name),
                });
            }
        };
        for parameter in values {
            result.push(ParameterContract {
                name: parameter.name.clone(),
                location: location_kind,
                // Why: RFC path labels cannot be omitted even if a declaration accidentally marks
                // one optional; the other locations retain TypeScript's optional-property meaning.
                required: location_kind == ParameterLocation::Path || !parameter.optional,
                style: style.to_owned(),
                explode,
                allow_reserved: false,
                schema: Some(schema::type_schema(ast, &parameter.value)?),
            });
        }
    }
    Ok(result)
}

fn request_contract(
    ast: &ContractAst,
    operation_id: &str,
    request: &TypeExpr,
) -> Result<RequestBodyContract> {
    let required = request
        .property("required")
        .and_then(TypeExpr::literal_bool)
        .unwrap_or(false);
    let content = request
        .property("content")
        .and_then(TypeExpr::properties)
        .ok_or_else(|| Error::InvalidOperation {
            operation: operation_id.to_owned(),
            detail: "request.content must be an object".to_owned(),
        })?;
    let examples = examples(ast, operation_id, "request", None);
    let content = content
        .iter()
        .map(|media| {
            Ok((
                media.name.clone(),
                MediaTypeContract {
                    schema: Some(schema::type_schema(ast, &media.value)?),
                    example: None,
                    examples: examples.clone(),
                    request_relation: None,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    Ok(RequestBodyContract { required, content })
}

fn response_contracts(
    ast: &ContractAst,
    operation_id: &str,
    definition: &TypeExpr,
) -> Result<Vec<ResponseContract>> {
    let responses = definition
        .property("responses")
        .and_then(TypeExpr::properties)
        .ok_or_else(|| Error::InvalidOperation {
            operation: operation_id.to_owned(),
            detail: "responses must be an object".to_owned(),
        })?;
    responses
        .iter()
        .map(|response| {
            let status = response_status(operation_id, &response.name)?;
            let content = response
                .value
                .property("content")
                .and_then(TypeExpr::properties)
                .map(|media| {
                    media
                        .iter()
                        .map(|media| {
                            Ok((
                                media.name.clone(),
                                MediaTypeContract {
                                    schema: Some(schema::type_schema(ast, &media.value)?),
                                    example: None,
                                    examples: examples(
                                        ast,
                                        operation_id,
                                        "responses",
                                        Some(&response.name),
                                    ),
                                    request_relation: response_relation(
                                        ast,
                                        operation_id,
                                        &response.name,
                                        &media.name,
                                    ),
                                },
                            ))
                        })
                        .collect::<Result<BTreeMap<_, _>>>()
                })
                .transpose()?
                .unwrap_or_default();
            let body = (!content.is_empty()).then_some(ResponseBodyContract { content });
            Ok(ResponseContract {
                status,
                definition: json!({}),
                body,
            })
        })
        .collect()
}

fn response_relation(
    ast: &ContractAst,
    operation_id: &str,
    status: &str,
    content_type: &str,
) -> Option<Value> {
    // A split declaration package owns response relations beside each resource's operations.
    // Looking through every public *ResponseRelations interface preserves that ownership while
    // still accepting a single aggregate ResponseRelations interface for compact contracts.
    let operation = ast
        .declarations
        .iter()
        .filter(|(name, _)| name.ends_with("ResponseRelations"))
        .find_map(|(_, relations)| relations.property(operation_id))?;
    operation
        .property(status)?
        .property(content_type)
        .and_then(|relation| schema::literal_value(ast, relation))
}

fn examples(
    ast: &ContractAst,
    operation_id: &str,
    direction: &str,
    status: Option<&str>,
) -> BTreeMap<String, Value> {
    let Some(operation) = ast
        .declarations
        .get("OperationExamples")
        .and_then(|examples| examples.property(operation_id))
    else {
        return BTreeMap::new();
    };
    let Some(mut witnesses) = operation.property(direction) else {
        return BTreeMap::new();
    };
    if let Some(status) = status {
        let Some(status_examples) = witnesses.property(status) else {
            return BTreeMap::new();
        };
        witnesses = status_examples;
    }
    witnesses
        .properties()
        .into_iter()
        .flatten()
        .filter_map(|example| {
            schema::literal_value(ast, &example.value).map(|value| (example.name.clone(), value))
        })
        .collect()
}

fn required_literal<'a>(
    definition: &'a TypeExpr,
    field: &str,
    operation_id: &str,
) -> Result<&'a str> {
    definition
        .property(field)
        .and_then(TypeExpr::literal_string)
        .ok_or_else(|| Error::InvalidOperation {
            operation: operation_id.to_owned(),
            detail: format!("{field} must be a string literal"),
        })
}

fn http_method(method: &str) -> HttpMethod {
    match method.to_ascii_uppercase().as_str() {
        "GET" => HttpMethod::Get,
        "PUT" => HttpMethod::Put,
        "POST" => HttpMethod::Post,
        "DELETE" => HttpMethod::Delete,
        "OPTIONS" => HttpMethod::Options,
        "HEAD" => HttpMethod::Head,
        "PATCH" => HttpMethod::Patch,
        "TRACE" => HttpMethod::Trace,
        "QUERY" => HttpMethod::Query,
        value => HttpMethod::Additional(value.to_owned()),
    }
}

fn response_status(operation: &str, value: &str) -> Result<ResponseStatus> {
    if value.eq_ignore_ascii_case("default") {
        return Ok(ResponseStatus::Default);
    }
    if let Some(hundreds) = value
        .strip_suffix("XX")
        .or_else(|| value.strip_suffix("xx"))
    {
        return hundreds
            .parse::<u16>()
            .map(ResponseStatus::Range)
            .map_err(|_| Error::InvalidOperation {
                operation: operation.to_owned(),
                detail: format!("invalid response status `{value}`"),
            });
    }
    value
        .parse::<u16>()
        .map(ResponseStatus::Exact)
        .map_err(|_| Error::InvalidOperation {
            operation: operation.to_owned(),
            detail: format!("invalid response status `{value}`"),
        })
}

#[cfg(test)]
mod tests {
    use super::{ParameterLocation, Source, module_specifiers, parse};

    #[test]
    fn parses_semantic_operation_and_literal_example() {
        let contract = parse(&[Source::new(
            "weather.d.ts",
            r#"
                export type CollectionVersion = "1.2.3";
                export interface WeatherOperations {
                  readonly getWeather: {
                    readonly method: "GET";
                    readonly path: "/weather";
                    readonly responses: {
                      readonly "200": { readonly content: {
                        readonly "application/json": { readonly condition: "sunny" | "rain" };
                      } };
                    };
                  };
                }
                export interface OperationExamples {
                  readonly getWeather: { readonly responses: { readonly "200": {
                    readonly sunny: { readonly condition: "sunny" };
                  } } };
                }
            "#,
        )])
        .unwrap_or_else(|error| panic!("contract must parse: {error}"));

        assert_eq!(contract.info.version, "1.2.3");
        let operation = contract
            .operation_by_id("getWeather")
            .unwrap_or_else(|| panic!("operation must exist"));
        assert_eq!(operation.path, "/weather");
        let body = operation.responses[0]
            .body
            .as_ref()
            .unwrap_or_else(|| panic!("response body must exist"));
        let examples = &body.content["application/json"].examples;
        assert_eq!(examples["sunny"]["condition"], "sunny");
    }

    #[test]
    fn lowers_semantic_soundness_and_response_relations() {
        let contract = parse(&[Source::new(
            "orders.d.ts",
            r#"
                export type ServiceName = "Orders";
                export type CollectionVersion = "1.0.0";
                export type Closed<Value> = Value;
                export type Ordered<Owner, Paths> = unknown;
                export interface Order {
                  readonly created_at: string;
                  readonly updated_at: string;
                  readonly id: string;
                }
                export interface CreateOrderRequest { readonly reference: string; }
                export interface SchemaSoundness {
                  readonly "Order": readonly [
                    Ordered<Order, readonly [readonly ["created_at"], readonly ["updated_at"]]>
                  ];
                }
                export interface OrdersOperations {
                  readonly createOrder: {
                    readonly method: "POST";
                    readonly path: "/orders";
                    readonly request: { readonly required: true; readonly content: {
                      readonly "application/json": Closed<CreateOrderRequest>;
                    } };
                    readonly responses: { readonly "201": { readonly content: {
                      readonly "application/json": Closed<{ readonly order: Order }>;
                    } } };
                  };
                }
                export interface ResponseRelations {
                  readonly createOrder: { readonly "201": { readonly "application/json": {
                    readonly bindings: readonly [{ readonly from: "/reference"; readonly to: "/order/id" }];
                  } } };
                }
            "#,
        )])
        .unwrap_or_else(|error| panic!("semantic contract must parse: {error}"));

        assert_eq!(
            contract.schemas["Order"]["x-simile-constraints"][0]["ordered"]["paths"],
            serde_json::json!(["/created_at", "/updated_at"])
        );
        let operation = contract
            .operation_by_id("createOrder")
            .unwrap_or_else(|| panic!("operation must exist"));
        let body = operation.responses[0]
            .body
            .as_ref()
            .unwrap_or_else(|| panic!("response body must exist"));
        let relation = body.content["application/json"]
            .request_relation
            .as_ref()
            .unwrap_or_else(|| panic!("relation must exist"));
        assert_eq!(relation["bindings"][0]["from"], "/reference");
    }

    #[test]
    fn finds_response_relations_owned_by_a_resource_module() {
        let contract = parse(&[Source::new(
            "orders.d.ts",
            r#"
                export interface OrdersOperations {
                  readonly createOrder: {
                    readonly method: "POST";
                    readonly path: "/orders";
                    readonly request: { readonly required: true; readonly content: {
                      readonly "application/json": { readonly reference: string };
                    } };
                    readonly responses: { readonly "201": { readonly content: {
                      readonly "application/json": { readonly order_id: string };
                    } } };
                  };
                }
                export interface OrdersResponseRelations {
                  readonly createOrder: { readonly "201": { readonly "application/json": {
                    readonly bindings: readonly [{
                      readonly from: "/reference";
                      readonly to: "/order_id";
                    }];
                  } } };
                }
                export interface ResponseRelations extends OrdersResponseRelations {}
            "#,
        )])
        .unwrap_or_else(|error| panic!("split semantic contract must parse: {error}"));

        let operation = contract
            .operation_by_id("createOrder")
            .unwrap_or_else(|| panic!("operation must exist"));
        let body = operation.responses[0]
            .body
            .as_ref()
            .unwrap_or_else(|| panic!("response body must exist"));
        let relation = body.content["application/json"]
            .request_relation
            .as_ref()
            .unwrap_or_else(|| panic!("resource-owned relation must exist"));
        assert_eq!(relation["bindings"][0]["to"], "/order_id");
    }

    #[test]
    fn lowers_distinct_values_and_dynamic_bound_sources() {
        let contract = parse(&[Source::new(
            "accounts.d.ts",
            r#"
                export type Field<Owner, Path> = unknown;
                export type Bounded<Owner, Path, Bounds> = unknown;
                export type DistinctValues<Owner, Source> = unknown;
                export interface Account {
                  readonly amount_due: number;
                  readonly amount_paid: number;
                  readonly lines: readonly { readonly id: string }[];
                }
                export interface SchemaSoundness {
                  readonly Account: readonly [
                    Bounded<Account, readonly ["amount_paid"], {
                      readonly maximum: Field<Account, readonly ["amount_due"]>;
                    }>,
                    DistinctValues<Account, readonly ["lines", "*", "id"]>
                  ];
                }
                export interface AccountsOperations {
                  readonly getAccount: {
                    readonly method: "GET";
                    readonly path: "/account";
                    readonly responses: { readonly "200": { readonly content: {
                      readonly "application/json": Account;
                    } } };
                  };
                }
            "#,
        )])
        .unwrap_or_else(|error| panic!("semantic constraints must parse: {error}"));

        let constraints = contract.schemas["Account"]["x-simile-constraints"]
            .as_array()
            .unwrap_or_else(|| panic!("constraint array must exist"));
        assert_eq!(constraints[0]["bounded"]["maximum"]["path"], "/amount_due");
        assert_eq!(constraints[1]["distinct"]["source"], "/lines/*/id");
    }

    #[test]
    fn rejects_relationship_members_with_different_selected_owners() {
        let result = parse(&[Source::new(
            "owners.d.ts",
            r#"
                export type Select<Owner, Path> = unknown;
                export type EqualsAt<Owner, Path, Expected> = unknown;
                export type Implies<When, Then> = unknown;
                export interface Owner {
                  readonly left: { readonly status: "open" | "closed" };
                  readonly right: { readonly status: "open" | "closed" };
                }
                export interface SchemaSoundness {
                  readonly Owner: readonly [
                    Implies<
                      EqualsAt<Select<Owner, readonly ["left"]>, readonly ["status"], "open">,
                      readonly [
                        EqualsAt<Select<Owner, readonly ["right"]>, readonly ["status"], "open">
                      ]
                    >
                  ];
                }
                export interface OwnersOperations {
                  readonly getOwner: {
                    readonly method: "GET";
                    readonly path: "/owner";
                    readonly responses: { readonly "200": { readonly content: {
                      readonly "application/json": Owner;
                    } } };
                  };
                }
            "#,
        )]);
        let error = match result {
            Ok(_) => panic!("mixed selected owners must fail"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("same selected owner"));
    }

    #[test]
    fn lowers_semantic_http_parameters() {
        let contract = parse(&[Source::new(
            "orders.d.ts",
            r#"
                export interface OrdersOperations {
                  readonly getOrder: {
                    readonly method: "GET";
                    readonly path: "/orders/{order_id}";
                    readonly parameters: {
                      readonly path: { readonly order_id: `or_${string}` };
                      readonly query: { readonly expand?: string };
                      readonly header: { readonly "X-Request-Id"?: string };
                      readonly cookie: Record<string, never>;
                    };
                    readonly responses: { readonly "200": { readonly content: {
                      readonly "application/json": { readonly id: string };
                    } } };
                  };
                }
            "#,
        )])
        .unwrap_or_else(|error| panic!("parameter contract must parse: {error}"));

        let operation = contract
            .operation_by_id("getOrder")
            .unwrap_or_else(|| panic!("operation must exist"));
        assert_eq!(operation.parameters.len(), 3);
        assert_eq!(operation.parameters[0].name, "order_id");
        assert_eq!(operation.parameters[0].location, ParameterLocation::Path);
        assert!(operation.parameters[0].required);
        assert_eq!(operation.parameters[1].location, ParameterLocation::Query);
        assert!(!operation.parameters[1].required);
        assert_eq!(operation.parameters[2].name, "X-Request-Id");
    }

    #[test]
    fn discovers_declaration_module_edges_without_execution() {
        let values = module_specifiers(Source::new(
            "commerce.d.ts",
            r#"
                import type { Input } from "./input";
                export * from "./operations";
                export type { Output } from "./output";
            "#,
        ))
        .unwrap_or_else(|error| panic!("module specifiers must parse: {error}"));
        assert_eq!(values, ["./input", "./operations", "./output"]);
    }
}
