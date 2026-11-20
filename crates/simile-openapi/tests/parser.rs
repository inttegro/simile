use simile_spec::{HttpMethod, OpenApiFeature, ParameterLocation, ResponseStatus, SecurityScheme};

#[test]
fn preserves_security_schemes_requirement_alternatives_and_public_overrides() {
    let source = r#"
openapi: 3.1.0
info: { title: Secured orders, version: 1.0.0 }
security:
  - merchantKey: []
    merchantOAuth: [orders:write]
  - bearerAuth: []
paths:
  /orders:
    post:
      operationId: createOrder
      responses:
        "201": { description: Created }
    get:
      operationId: listOrders
      security: []
      responses:
        "200": { description: Listed }
components:
  securitySchemes:
    merchantKey:
      type: apiKey
      in: header
      name: X-Merchant-Key
    merchantOAuth:
      type: oauth2
      flows: {}
    bearerAuth:
      type: http
      scheme: bearer
      bearerFormat: JWT
"#;
    let contract = simile_openapi::parse(source).unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(
        contract.security_schemes["merchantKey"],
        SecurityScheme::ApiKey {
            location: ParameterLocation::Header,
            name: "X-Merchant-Key".to_owned(),
        }
    );
    assert_eq!(
        contract.security_schemes["bearerAuth"],
        SecurityScheme::Http {
            scheme: "bearer".to_owned(),
            bearer_format: Some("JWT".to_owned()),
        }
    );
    let secured = contract
        .operation_by_id("createOrder")
        .unwrap_or_else(|| panic!("create operation"));
    assert_eq!(secured.security.len(), 2);
    assert_eq!(
        secured.security[0].schemes["merchantOAuth"],
        ["orders:write"]
    );
    assert!(
        contract
            .operation_by_id("listOrders")
            .unwrap_or_else(|| panic!("list operation"))
            .security
            .is_empty()
    );
}

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
    assert_eq!(
        body.content["application/json"].examples["created"],
        serde_json::json!({"id": "ord_123"})
    );
}

#[test]
fn merges_path_and_operation_parameters_with_operation_precedence() {
    let source = r#"
openapi: 3.1.2
info: { title: Orders, version: 1.0.0 }
paths:
  /orders/{id}:
    parameters:
      - $ref: '#/components/parameters/OrderId'
      - in: header
        name: X-Trace
        schema: { type: string, minLength: 4 }
    get:
      operationId: getOrder
      parameters:
        - in: header
          name: x-trace
          required: true
          schema: { type: string, minLength: 12 }
        - in: query
          name: expand
          schema:
            type: array
            items: { type: string }
      responses:
        "200": { description: Found }
components:
  parameters:
    OrderId:
      in: path
      name: id
      required: true
      schema: { type: string, pattern: '^ord_' }
"#;
    let contract = simile_openapi::parse(source).unwrap_or_else(|error| panic!("{error}"));
    let operation = contract
        .operation_by_id("getOrder")
        .unwrap_or_else(|| panic!("operation missing"));

    assert_eq!(operation.parameters.len(), 3);
    let id = operation
        .parameters
        .iter()
        .find(|parameter| parameter.name == "id")
        .unwrap_or_else(|| panic!("id parameter"));
    assert_eq!(id.location, ParameterLocation::Path);
    assert!(id.required);
    assert_eq!(id.style, "simple");

    let trace = operation
        .parameters
        .iter()
        .find(|parameter| parameter.name.eq_ignore_ascii_case("x-trace"))
        .unwrap_or_else(|| panic!("trace parameter"));
    assert!(trace.required);
    assert_eq!(
        trace
            .schema
            .as_ref()
            .and_then(|schema| schema["minLength"].as_u64()),
        Some(12)
    );

    let expand = operation
        .parameters
        .iter()
        .find(|parameter| parameter.name == "expand")
        .unwrap_or_else(|| panic!("expand parameter"));
    assert_eq!(expand.location, ParameterLocation::Query);
    assert_eq!(expand.style, "form");
    assert!(expand.explode);
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

#[test]
fn normalizes_callbacks_and_top_level_webhooks() {
    let source = r#"
openapi: 3.1.2
info: { title: Payments, version: 1.0.0 }
paths:
  /payments:
    post:
      operationId: createPayment
      requestBody:
        content:
          application/json:
            schema:
              type: object
              properties:
                callback_url: { type: string, format: uri }
      callbacks:
        status:
          '{$request.body#/callback_url}':
            post:
              operationId: paymentStatusChanged
              requestBody:
                required: true
                content:
                  application/json:
                    schema:
                      $ref: '#/components/schemas/PaymentEvent'
              responses:
                "204": { description: Accepted }
      responses:
        "201": { description: Created }
webhooks:
  account.updated:
    post:
      requestBody:
        required: true
        content:
          application/json:
            schema: { type: object, required: [id] }
      responses:
        "204": { description: Accepted }
components:
  schemas:
    PaymentEvent:
      type: object
      required: [id, status]
"#;

    let contract = simile_openapi::parse(source).unwrap_or_else(|error| panic!("{error}"));
    let callback = &contract.webhooks["paymentStatusChanged"];
    assert_eq!(callback.method, HttpMethod::Post);
    assert_eq!(
        callback.target.as_deref(),
        Some("{$request.body#/callback_url}")
    );
    assert_eq!(callback.origin, simile_spec::WebhookOrigin::OpenApiCallback);
    assert!(callback.request.is_some());

    let webhook = &contract.webhooks["account.updated"];
    assert_eq!(webhook.method, HttpMethod::Post);
    assert_eq!(webhook.origin, simile_spec::WebhookOrigin::OpenApiWebhook);
    assert!(webhook.target.is_none());
}

#[test]
fn openapi_3_0_ignores_the_unsupported_top_level_webhooks_field() {
    let source = r#"
openapi: 3.0.4
info: { title: Payments, version: 1.0.0 }
paths: {}
webhooks:
  payment.updated:
    post:
      responses:
        "204": { description: Accepted }
"#;
    let contract = simile_openapi::parse(source).unwrap_or_else(|error| panic!("{error}"));
    assert!(contract.webhooks.is_empty());
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
