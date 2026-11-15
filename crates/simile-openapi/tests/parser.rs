use simile_spec::{HttpMethod, OpenApiFeature, ResponseStatus};

#[test]
fn normalizes_the_three_supported_feature_lines() {
    let cases = [
        ("3.0.4", OpenApiFeature::V3_0),
        ("3.1.2", OpenApiFeature::V3_1),
        ("3.2.1", OpenApiFeature::V3_2),
    ];

    for (version, feature) in cases {
        let source = fixture(version, "post");
        let contract = simile_openapi::parse(&source).unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(contract.openapi.exact, version);
        assert_eq!(contract.openapi.feature, feature);
        let operation = contract
            .operation_by_id("createOrder")
            .unwrap_or_else(|| panic!("operation not found for {version}"));
        assert_eq!(operation.method, HttpMethod::Post);
        assert_eq!(operation.path, "/orders");
        assert_eq!(operation.responses[0].status, ResponseStatus::Exact(201));
    }
}

#[test]
fn accepts_json_and_yaml_documents() {
    let yaml = fixture("3.1.2", "post");
    let json = serde_json::json!({
        "openapi": "3.1.2",
        "info": { "title": "Orders", "version": "1.0.0" },
        "paths": {
            "/orders": {
                "post": {
                    "operationId": "createOrder",
                    "responses": { "201": { "description": "Created" } }
                }
            }
        }
    })
    .to_string();

    let yaml_contract = simile_openapi::parse(&yaml).unwrap_or_else(|error| panic!("{error}"));
    let json_contract = simile_openapi::parse(&json).unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(yaml_contract, json_contract);
}

#[test]
fn normalizes_request_bodies_and_component_schemas() {
    let source = r#"
openapi: 3.1.2
info:
  title: Orders
  version: 1.0.0
paths:
  /orders:
    post:
      operationId: createOrder
      requestBody:
        $ref: '#/components/requestBodies/CreateOrder'
      responses:
        "201": { description: Created }
components:
  requestBodies:
    CreateOrder:
      required: true
      content:
        application/json:
          examples:
            minimal:
              value: { customerId: cus_123 }
          schema:
            $ref: '#/components/schemas/OrderInput'
  schemas:
    OrderInput:
      type: object
      required: [customerId]
"#;
    let contract = simile_openapi::parse(source).unwrap_or_else(|error| panic!("{error}"));
    let operation = contract
        .operation_by_id("createOrder")
        .unwrap_or_else(|| panic!("operation missing"));
    let request = operation
        .request
        .as_ref()
        .unwrap_or_else(|| panic!("request body missing"));

    assert!(request.required);
    assert_eq!(
        request.content["application/json"].schema,
        Some(serde_json::json!({"$ref": "#/components/schemas/OrderInput"}))
    );
    assert_eq!(
        request.content["application/json"].example,
        Some(serde_json::json!({"customerId": "cus_123"}))
    );
    assert_eq!(contract.schemas["OrderInput"]["type"], "object");
}

#[test]
fn normalizes_referenced_response_bodies_and_examples() {
    let source = r#"
openapi: 3.1.2
info: { title: Orders, version: 1.0.0 }
paths:
  /orders:
    post:
      operationId: createOrder
      responses:
        "201":
          $ref: '#/components/responses/CreatedOrder'
components:
  responses:
    CreatedOrder:
      description: Created
      content:
        application/json:
          examples:
            created:
              value: { id: ord_123 }
          schema:
            $ref: '#/components/schemas/Order'
  schemas:
    Order:
      type: object
      required: [id]
      properties:
        id: { type: string }
"#;
    let contract = simile_openapi::parse(source).unwrap_or_else(|error| panic!("{error}"));
    let operation = contract
        .operation_by_id("createOrder")
        .unwrap_or_else(|| panic!("operation missing"));
    let body = operation.responses[0]
        .body
        .as_ref()
        .unwrap_or_else(|| panic!("response body missing"));

    assert_eq!(
        body.content["application/json"].schema,
        Some(serde_json::json!({"$ref": "#/components/schemas/Order"}))
    );
    assert_eq!(
        body.content["application/json"].example,
        Some(serde_json::json!({"id": "ord_123"}))
    );
}

#[test]
fn query_is_only_an_operation_in_openapi_3_2() {
    let old =
        simile_openapi::parse(&fixture("3.1.2", "query")).unwrap_or_else(|error| panic!("{error}"));
    let current =
        simile_openapi::parse(&fixture("3.2.1", "query")).unwrap_or_else(|error| panic!("{error}"));

    assert!(old.operations.is_empty());
    assert_eq!(current.operations.len(), 1);
    assert_eq!(
        current
            .operations
            .values()
            .next()
            .map(|value| value.method.clone()),
        Some(HttpMethod::Query)
    );
}

#[test]
fn openapi_3_2_preserves_additional_http_operations() {
    let source = r#"
openapi: 3.2.1
info:
  title: Cache
  version: 1.0.0
paths:
  /cache:
    additionalOperations:
      PURGE:
        operationId: purgeCache
        responses:
          "204": { description: Purged }
"#;
    let contract = simile_openapi::parse(source).unwrap_or_else(|error| panic!("{error}"));
    let operation = contract
        .operation_by_id("purgeCache")
        .unwrap_or_else(|| panic!("operation missing"));

    assert_eq!(operation.method, HttpMethod::Additional("PURGE".to_owned()));
    assert_eq!(operation.key.as_str(), "PURGE /cache");
}

#[test]
fn preserves_response_ranges_and_defaults() {
    let source = r#"
openapi: 3.2.1
info:
  title: Orders
  version: 1.0.0
paths:
  /orders:
    get:
      responses:
        "2XX": { description: Any success }
        default: { description: Any other result }
"#;
    let contract = simile_openapi::parse(source).unwrap_or_else(|error| panic!("{error}"));
    let operation = contract
        .operations
        .values()
        .next()
        .unwrap_or_else(|| panic!("operation missing"));

    assert_eq!(operation.responses[0].status, ResponseStatus::Range(2));
    assert_eq!(operation.responses[1].status, ResponseStatus::Default);
    assert!(operation.declares_status(204));
    assert!(operation.declares_status(503));
}

#[test]
fn rejects_unsupported_versions() {
    let error = parse_error(&fixture("2.0.0", "post"));

    assert!(error.to_string().contains("unsupported"));
}

#[test]
fn accepts_future_patch_releases_in_a_supported_feature_line() {
    let contract =
        simile_openapi::parse(&fixture("3.1.99", "post")).unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(contract.openapi.exact, "3.1.99");
    assert_eq!(contract.openapi.feature, OpenApiFeature::V3_1);
}

#[test]
fn rejects_duplicate_operation_ids() {
    let source = r#"
openapi: 3.1.2
info:
  title: Orders
  version: 1.0.0
paths:
  /orders:
    post:
      operationId: writeOrder
      responses:
        "201": { description: Created }
  /orders/{id}:
    put:
      operationId: writeOrder
      responses:
        "200": { description: Updated }
"#;

    let error = parse_error(source);

    assert!(error.to_string().contains("writeOrder"));
    assert!(error.to_string().contains("POST /orders"));
    assert!(error.to_string().contains("PUT /orders/{id}"));
}

fn fixture(version: &str, method: &str) -> String {
    format!(
        r#"
openapi: {version}
info:
  title: Orders
  version: 1.0.0
paths:
  /orders:
    {method}:
      operationId: createOrder
      responses:
        "201":
          description: Created
"#
    )
}

fn parse_error(source: &str) -> simile_openapi::Error {
    match simile_openapi::parse(source) {
        Err(error) => error,
        Ok(contract) => panic!("expected parsing to fail, compiled {contract:?}"),
    }
}
