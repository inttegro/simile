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
fn proves_conditional_relationships_from_openapi() {
    let artifact = artifact();
    let error = match validate_request_body(
        &artifact,
        IncomingBody {
            operation: "POST /orders",
            content_type: Some("application/json"),
            bytes: br#"{
                "customer": {"email": "buyer@example.com"},
                "items": [{"quantity": 1}],
                "active": false,
                "published_at": "2026-01-01T00:00:00Z",
                "archived_at": null,
                "checkout_enabled": true
            }"#,
        },
    ) {
        Err(error) => error,
        Ok(body) => panic!("unsound body should be rejected, got {body:?}"),
    };

    let issues = error.issues();
    assert_eq!(issues.len(), 3);
    assert_eq!(issues[0].path, "/published_at");
    assert_eq!(issues[0].rule.to_string(), "implies");
    assert_eq!(issues[1].path, "/archived_at");
    assert_eq!(issues[1].rule.to_string(), "implies");
    assert_eq!(issues[2].path, "/checkout_enabled");
    assert_eq!(issues[2].rule.to_string(), "implies");
}

#[test]
fn proves_compiled_equations_aggregates_templates_and_lifecycles() {
    let artifact = soundness_artifact();
    let error = match validate_request_body(
        &artifact,
        IncomingBody {
            operation: "POST /invoices",
            content_type: Some("application/json"),
            bytes: br#"{
                "id": "in_123",
                "url": "/wrong",
                "currency": "usd",
                "lines": [
                    {"amount": 5, "currency": "gbp"},
                    {"amount": 7, "currency": "usd"}
                ],
                "subtotal": 11,
                "amount_due": 30,
                "amount_paid": 12,
                "amount_remaining": 14,
                "status": "paid",
                "created": 1,
                "status_transitions": {
                    "finalized_at": null,
                    "paid_at": 3,
                    "voided_at": 2
                }
            }"#,
        },
    ) {
        Err(error) => error,
        Ok(body) => panic!("unsound body should be rejected, got {body:?}"),
    };
    let rules = error
        .issues()
        .iter()
        .map(|issue| issue.rule.to_string())
        .collect::<Vec<_>>();
    assert!(rules.contains(&"equal".to_owned()));
    assert!(rules.contains(&"equation".to_owned()));
    assert!(rules.contains(&"aggregate".to_owned()));
    assert!(rules.contains(&"template".to_owned()));
    assert!(rules.contains(&"lifecycle".to_owned()));

    for seed in 0..16 {
        let body = synthesize_request_body_with_seed(
            &artifact,
            SynthesisRequest {
                operation: "POST /invoices",
                content_type: Some("application/json"),
            },
            seed,
        )
        .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        validate_request_body(
            &artifact,
            IncomingBody {
                operation: "POST /invoices",
                content_type: Some(body.media_type()),
                bytes: body.bytes(),
            },
        )
        .unwrap_or_else(|error| panic!("seed {seed}: {error}"))
        .unwrap_or_else(|| panic!("seed {seed}: body should be present"));
    }
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
        active: { type: boolean }
        archived_at: { type: [string, "null"], format: date-time }
        checkout_enabled: { type: boolean }
      x-simile-constraints:
        - ordered:
            paths: [/created_at, /published_at, /updated_at]
        - implies:
            when: { path: /active, equals: false }
            then:
              all:
                - { path: /published_at, state: null-or-absent }
                - { path: /archived_at, state: non-null }
                - { path: /checkout_enabled, equals: false }
"#;
    let service = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("orders.yaml", source),
            modules: Vec::new(),
            heuristic_policy: simile_heuristics::Policy::Standard,
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
            modules: Vec::new(),
            heuristic_policy: simile_heuristics::Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    decode(&encode(&service).unwrap_or_else(|error| panic!("{error}")))
        .unwrap_or_else(|error| panic!("{error}"))
}

fn soundness_artifact() -> simile_artifact::Artifact {
    let source = r#"
openapi: 3.1.2
info: { title: Invoices, version: 1.0.0 }
paths:
  /invoices:
    post:
      operationId: createInvoice
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/Invoice'
      responses:
        "201": { description: Created }
components:
  schemas:
    Invoice:
      type: object
      additionalProperties: false
      required:
        [id, url, currency, lines, subtotal, amount_due, amount_paid, amount_remaining,
         status, created, status_transitions]
      properties:
        id: { type: string }
        url: { type: string }
        currency: { type: string, enum: [usd, gbp] }
        lines:
          type: array
          minItems: 1
          items:
            type: object
            required: [amount, currency]
            properties:
              amount: { type: integer }
              currency: { type: string, enum: [usd, gbp] }
        subtotal: { type: integer }
        amount_due: { type: integer }
        amount_paid: { type: integer }
        amount_remaining: { type: integer }
        status: { type: string, enum: [draft, open, paid, void] }
        created: { type: integer }
        status_transitions:
          type: object
          required: [finalized_at, paid_at, voided_at]
          properties:
            finalized_at: { type: [integer, "null"] }
            paid_at: { type: [integer, "null"] }
            voided_at: { type: [integer, "null"] }
      x-simile-constraints:
        - equal:
            paths: [/currency, /lines/*/currency]
        - equation:
            target: /amount_remaining
            value:
              subtract:
                - { path: /amount_due }
                - { path: /amount_paid }
        - aggregate:
            target: /subtotal
            operation: sum
            source: /lines/*/amount
        - template:
            path: /url
            value: /v1/invoices/{/id}/lines
        - lifecycle:
            state: /status
            created-at: /created
            transitions:
              - { from: draft, to: open, at: /status_transitions/finalized_at }
              - { from: open, to: paid, at: /status_transitions/paid_at }
              - { from: open, to: void, at: /status_transitions/voided_at }
"#;
    let service = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("invoices.yaml", source),
            modules: Vec::new(),
            heuristic_policy: simile_heuristics::Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    decode(&encode(&service).unwrap_or_else(|error| panic!("{error}")))
        .unwrap_or_else(|error| panic!("{error}"))
}
