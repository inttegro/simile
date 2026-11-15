use serde_json::json;
use simile_artifact::{decode, encode};
use simile_compiler::{CompileRequest, Compiler, Source};
use simile_runtime::{Error, IncomingBody, validate_request_body};

#[test]
fn validates_nested_json_and_returns_structured_paths() {
    let artifact = artifact();
    let valid = br#"{"customer":{"email":"buyer@example.com"},"items":[{"quantity":2}]}"#;
    let body = validate_request_body(
        &artifact,
        IncomingBody {
            operation: "POST /orders",
            content_type: Some("application/json; charset=utf-8"),
            bytes: valid,
        },
    )
    .unwrap_or_else(|error| panic!("{error}"))
    .unwrap_or_else(|| panic!("body should be present"));
    assert_eq!(body.value()["items"][0]["quantity"], 2);

    let invalid = br#"{"customer":{},"items":[{"quantity":0}],"debug":true}"#;
    let error = match validate_request_body(
        &artifact,
        IncomingBody {
            operation: "POST /orders",
            content_type: Some("application/json"),
            bytes: invalid,
        },
    ) {
        Err(error) => error,
        Ok(body) => panic!("body should fail validation, got {body:?}"),
    };
    let paths = error
        .issues()
        .iter()
        .map(|issue| issue.path.as_str())
        .collect::<Vec<_>>();
    assert!(paths.contains(&"/customer/email"));
    assert!(paths.contains(&"/items/0/quantity"));
    assert!(paths.contains(&"/debug"));
}

#[test]
fn distinguishes_protocol_and_schema_failures() {
    let artifact = artifact();
    let missing = validate_request_body(
        &artifact,
        IncomingBody {
            operation: "POST /orders",
            content_type: None,
            bytes: b"",
        },
    );
    assert!(matches!(missing, Err(Error::BodyRequired { .. })));

    let malformed = validate_request_body(
        &artifact,
        IncomingBody {
            operation: "POST /orders",
            content_type: Some("application/json"),
            bytes: br#"{"#,
        },
    );
    assert!(matches!(malformed, Err(Error::MalformedJson { .. })));

    let unsupported = validate_request_body(
        &artifact,
        IncomingBody {
            operation: "POST /orders",
            content_type: Some("text/plain"),
            bytes: b"order",
        },
    );
    assert!(matches!(
        unsupported,
        Err(Error::UnsupportedMediaType { .. })
    ));
}

fn artifact() -> simile_artifact::Artifact {
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
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/OrderInput'
      responses:
        "201": { description: Created }
components:
  schemas:
    OrderInput:
      type: object
      additionalProperties: false
      required: [customer, items]
      properties:
        customer:
          type: object
          required: [email]
          properties:
            email: { type: string, minLength: 3 }
        items:
          type: array
          minItems: 1
          items:
            type: object
            required: [quantity]
            properties:
              quantity: { type: integer, minimum: 1 }
"#;
    let service = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("orders.yaml", source),
            behaviors: Vec::new(),
        })
        .unwrap_or_else(|error| panic!("{error}"));
    let encoded = encode(&service).unwrap_or_else(|error| panic!("{error}"));
    let artifact = decode(&encoded).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(artifact.summary().request_bodies, 1);
    assert_eq!(
        artifact
            .operation("POST /orders")
            .and_then(|operation| operation.request_body())
            .map(|request| request.is_required()),
        Some(true)
    );
    assert_eq!(json!(artifact.summary().operations), json!(1));
    artifact
}
