use serde_json::json;
use simile_artifact::{decode, encode};
use simile_compiler::{CompileRequest, Compiler, Source};
use simile_runtime::{
    Error, IncomingBody, SynthesisRequest, synthesize_request_body,
    synthesize_request_body_with_seed, validate_request_body,
};

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

#[test]
fn proves_cross_field_relationships_from_openapi() {
    let artifact = artifact();
    let error = match validate_request_body(
        &artifact,
        IncomingBody {
            operation: "POST /orders",
            content_type: Some("application/json"),
            bytes: br#"{
                "customer": {"email": "buyer@example.com"},
                "items": [{"quantity": 1}],
                "created_at": "2026-01-03T00:00:00Z",
                "published_at": "2026-01-02T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z"
            }"#,
        },
    ) {
        Err(error) => error,
        Ok(body) => panic!("unsound body should be rejected, got {body:?}"),
    };

    let issues = error.issues();
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].path, "/published_at");
    assert_eq!(issues[0].rule.to_string(), "ordered");
    assert_eq!(issues[1].path, "/updated_at");
    assert_eq!(issues[1].rule.to_string(), "ordered");
}

#[test]
fn synthesizes_a_body_that_round_trips_through_validation() {
    let artifact = artifact();
    let body = synthesize_request_body(
        &artifact,
        SynthesisRequest {
            operation: "POST /orders",
            content_type: None,
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(body.media_type(), "application/json");
    assert!(
        body.value()["customer"]["email"]
            .as_str()
            .is_some_and(|email| email.len() >= 3)
    );
    assert!(
        body.value()["items"][0]["quantity"]
            .as_i64()
            .is_some_and(|quantity| quantity >= 1)
    );
    let validated = validate_request_body(
        &artifact,
        IncomingBody {
            operation: "POST /orders",
            content_type: Some(body.media_type()),
            bytes: body.bytes(),
        },
    )
    .unwrap_or_else(|error| panic!("{error}"))
    .unwrap_or_else(|| panic!("synthesized body should be present"));
    assert_eq!(validated.value(), body.value());
}

#[test]
fn fuzz_synthesis_is_not_pinned_to_a_declared_example() {
    let artifact = artifact_with_example();
    let first = synthesize_request_body_with_seed(
        &artifact,
        SynthesisRequest {
            operation: "POST /messages",
            content_type: Some("application/json"),
        },
        41,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let second = synthesize_request_body_with_seed(
        &artifact,
        SynthesisRequest {
            operation: "POST /messages",
            content_type: Some("application/json"),
        },
        42,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let replay = synthesize_request_body_with_seed(
        &artifact,
        SynthesisRequest {
            operation: "POST /messages",
            content_type: Some("application/json"),
        },
        41,
    )
    .unwrap_or_else(|error| panic!("{error}"));

    assert_ne!(first.value(), &json!({"text": "hello"}));
    assert_ne!(first.value(), second.value());
    assert_eq!(first.value(), replay.value());
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
        created_at: { type: string, format: date-time }
        published_at: { type: [string, "null"], format: date-time }
        updated_at: { type: string, format: date-time }
      x-simile-constraints:
        - ordered:
            paths: [/created_at, /published_at, /updated_at]
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

fn artifact_with_example() -> simile_artifact::Artifact {
    let source = r#"
openapi: 3.1.2
info: { title: Messages, version: 1.0.0 }
paths:
  /messages:
    post:
      requestBody:
        required: true
        content:
          application/json:
            example: { text: hello }
            schema:
              type: object
              additionalProperties: false
              required: [text]
              properties:
                text: { type: string, minLength: 1 }
      responses:
        "201": { description: Created }
"#;
    let service = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("messages.yaml", source),
            behaviors: Vec::new(),
        })
        .unwrap_or_else(|error| panic!("{error}"));
    decode(&encode(&service).unwrap_or_else(|error| panic!("{error}")))
        .unwrap_or_else(|error| panic!("{error}"))
}
