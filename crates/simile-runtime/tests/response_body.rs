use serde_json::json;
use simile_artifact::{decode, encode};
use simile_compiler::{CompileRequest, Compiler, Source};
use simile_runtime::{
    Error, IncomingBody, ResponseSynthesisRequest, synthesize_response, validate_request_body,
};

#[test]
fn synthesizes_a_valid_response_without_request_context() {
    let artifact = artifact();
    let response = synthesize_response(
        &artifact,
        ResponseSynthesisRequest {
            operation: "POST /orders",
            status: None,
            content_type: None,
            request_body: None,
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(response.status(), 201);
    assert_eq!(response.media_type(), Some("application/json"));
    assert!(
        response
            .value()
            .and_then(|value| value.get("id"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|id| !id.is_empty())
    );
    assert_eq!(
        response.value().and_then(|value| value.get("status")),
        Some(&json!("pending"))
    );
    assert!(
        response
            .value()
            .is_some_and(|value| value.get("secret").is_none())
    );
    let decoded: serde_json::Value = serde_json::from_slice(response.bytes())
        .unwrap_or_else(|error| panic!("response should be JSON: {error}"));
    assert_eq!(response.value(), Some(&decoded));
}

#[test]
fn preserves_explicit_formats_in_the_compiled_artifact() {
    let openapi = r#"
openapi: 3.0.0
info: { title: Events, version: 1.0.0 }
paths:
  /events:
    post:
      responses:
        "200":
          description: Created
          content:
            application/json:
              schema:
                type: object
                required: [created, available_at, note]
                properties:
                  created:
                    type: integer
                    format: unix-time
                    description: Seconds since the Unix epoch
                  available_at: { type: string, format: date-time }
                  note:
                    type: string
                    description: An email address
"#;
    let artifact = compile_artifact(openapi, None);
    let response = synthesize_response(
        &artifact,
        ResponseSynthesisRequest {
            operation: "POST /events",
            status: None,
            content_type: None,
            request_body: None,
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let value = response
        .value()
        .unwrap_or_else(|| panic!("response body should be present"));

    assert!(value["created"].as_i64().is_some());
    assert!(
        value["available_at"]
            .as_str()
            .is_some_and(|value| value.contains('T') && value.ends_with('Z'))
    );
    assert!(
        value["note"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
}

#[test]
fn carries_compatible_request_values_into_the_response() {
    let artifact = artifact();
    let request = validate_request_body(
        &artifact,
        IncomingBody {
            operation: "POST /orders",
            content_type: Some("application/json"),
            bytes: br#"{
                "customer": {"email": "buyer@example.com"},
                "amount": 4200,
                "secret": "do-not-reflect"
            }"#,
        },
    )
    .unwrap_or_else(|error| panic!("{error}"))
    .unwrap_or_else(|| panic!("request body should be present"));
    let response = synthesize_response(
        &artifact,
        ResponseSynthesisRequest {
            operation: "POST /orders",
            status: Some(201),
            content_type: Some("application/json; charset=utf-8"),
            request_body: Some(&request),
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let value = response
        .value()
        .unwrap_or_else(|| panic!("response body should be present"));

    assert_eq!(value["customer"]["email"], "buyer@example.com");
    assert_eq!(value["customer"]["loyalty"], "new");
    assert_eq!(value["amount"], 4200);
    assert!(value["id"].as_str().is_some_and(|id| !id.is_empty()));
    assert_eq!(value["status"], "pending");
    assert!(value.get("secret").is_none());
}

#[test]
fn returns_bodyless_responses_without_inventing_content() {
    let artifact = artifact();
    let response = synthesize_response(
        &artifact,
        ResponseSynthesisRequest {
            operation: "POST /orders",
            status: Some(204),
            content_type: None,
            request_body: None,
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(response.status(), 204);
    assert_eq!(response.media_type(), None);
    assert!(response.bytes().is_empty());
    assert_eq!(response.value(), None);
}

#[test]
fn rejects_request_context_from_another_operation() {
    let artifact = artifact();
    let request = validate_request_body(
        &artifact,
        IncomingBody {
            operation: "POST /orders",
            content_type: Some("application/json"),
            bytes: br#"{
                "customer": {"email": "buyer@example.com"},
                "amount": 4200,
                "secret": "do-not-reflect"
            }"#,
        },
    )
    .unwrap_or_else(|error| panic!("{error}"))
    .unwrap_or_else(|| panic!("request body should be present"));
    let result = synthesize_response(
        &artifact,
        ResponseSynthesisRequest {
            operation: "GET /reports",
            status: None,
            content_type: None,
            request_body: Some(&request),
        },
    );

    assert!(matches!(result, Err(Error::RequestContextMismatch { .. })));
}

#[test]
fn honors_hcl_status_selection() {
    let openapi = r#"
openapi: 3.1.2
info: { title: Jobs, version: 1.0.0 }
paths:
  /jobs:
    post:
      operationId: startJob
      responses:
        "201": { description: Completed }
        "202": { description: Accepted }
"#;
    let behavior = r#"
behavior "startJob" {
  response { status = 202 }
}
"#;
    let artifact = compile_artifact(openapi, Some(behavior));
    let response = synthesize_response(
        &artifact,
        ResponseSynthesisRequest {
            operation: "POST /jobs",
            status: None,
            content_type: None,
            request_body: None,
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(response.status(), 202);
}

#[test]
fn resolves_response_ranges_before_default() {
    let openapi = r#"
openapi: 3.1.2
info: { title: Jobs, version: 1.0.0 }
paths:
  /jobs:
    post:
      responses:
        "2XX": { description: Success }
        default: { description: Failure }
"#;
    let artifact = compile_artifact(openapi, None);

    for (requested, expected) in [(None, 200), (Some(207), 207), (Some(503), 503)] {
        let response = synthesize_response(
            &artifact,
            ResponseSynthesisRequest {
                operation: "POST /jobs",
                status: requested,
                content_type: None,
                request_body: None,
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(response.status(), expected);
    }
}

#[test]
fn gives_a_default_only_response_a_concrete_status() {
    let openapi = r#"
openapi: 3.1.2
info: { title: Jobs, version: 1.0.0 }
paths:
  /jobs:
    post:
      responses:
        default: { description: Unspecified outcome }
"#;
    let artifact = compile_artifact(openapi, None);
    let response = synthesize_response(
        &artifact,
        ResponseSynthesisRequest {
            operation: "POST /jobs",
            status: None,
            content_type: None,
            request_body: None,
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(response.status(), 500);
}

fn artifact() -> simile_artifact::Artifact {
    let source = r#"
openapi: 3.1.2
info: { title: Orders, version: 1.0.0 }
paths:
  /orders:
    post:
      operationId: createOrder
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/OrderInput'
      responses:
        "201":
          description: Created
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/Order'
        "204": { description: No content }
  /reports:
    get:
      responses:
        "200": { description: Ready }
components:
  schemas:
    OrderInput:
      type: object
      additionalProperties: false
      required: [customer, amount, secret]
      properties:
        customer:
          type: object
          additionalProperties: false
          required: [email]
          properties:
            email: { type: string, minLength: 3 }
        amount: { type: integer, minimum: 1 }
        secret: { type: string, writeOnly: true }
    Order:
      type: object
      additionalProperties: false
      required: [id, customer, amount, status, created, secret]
      properties:
        id: { type: string, example: ord_simile }
        customer:
          type: object
          additionalProperties: false
          required: [email, loyalty]
          properties:
            email: { type: string, minLength: 3 }
            loyalty: { type: string, enum: [new] }
        amount: { type: integer, minimum: 1 }
        status: { type: string, enum: [pending] }
        created: { type: integer, minimum: 1 }
        secret: { type: string, writeOnly: true }
"#;
    compile_artifact(source, None)
}

fn compile_artifact(openapi: &str, behavior: Option<&str>) -> simile_artifact::Artifact {
    let behaviors = behavior
        .map(|contents| vec![Source::new("behavior.hcl", contents)])
        .unwrap_or_default();
    let service = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("service.yaml", openapi),
            behaviors,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    decode(&encode(&service).unwrap_or_else(|error| panic!("{error}")))
        .unwrap_or_else(|error| panic!("{error}"))
}
