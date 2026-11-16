use serde_json::json;
use simile_artifact::{Artifact, decode, encode};
use simile_compiler::{CompileRequest, Compiler, Source};
use simile_runtime::{
    Error, ExecutionOptions, OperationCall, OperationValueCall, ReplayIdentity, Runtime,
};

#[test]
fn runs_an_operation_through_validation_response_and_effect_planning() {
    let artifact = artifact();
    let result = Runtime::new(&artifact)
        .execute(OperationCall {
            operation: "POST /orders",
            content_type: Some("application/json"),
            body: br#"{"product":"beat_pack","quantity":2}"#,
        })
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(result.operation(), "POST /orders");
    assert_eq!(
        result
            .request_body()
            .and_then(|body| body.value().get("product")),
        Some(&json!("beat_pack"))
    );
    assert_eq!(result.response().status(), 201);
    assert_eq!(
        result
            .response()
            .value()
            .and_then(|body| body.get("product")),
        Some(&json!("beat_pack"))
    );
    assert_eq!(
        result
            .response()
            .value()
            .and_then(|body| body.get("quantity")),
        Some(&json!(2))
    );
    assert_eq!(
        result
            .async_operations()
            .iter()
            .map(|operation| operation.name())
            .collect::<Vec<_>>(),
        vec!["record_order", "send_receipt"]
    );
    assert_eq!(
        result.async_operations()[0].resources()[0].as_str(),
        "db.orders"
    );
    assert_eq!(
        result.async_operations()[1].resources()[0].as_str(),
        "mail.receipts"
    );
}

#[test]
fn rejects_an_invalid_request_before_producing_an_outcome() {
    let artifact = artifact();
    let result = Runtime::new(&artifact).execute(OperationCall {
        operation: "POST /orders",
        content_type: Some("application/json"),
        body: br#"{"product":"beat_pack","quantity":0}"#,
    });

    assert!(matches!(result, Err(Error::InvalidBody { .. })));
}

#[test]
fn runs_openapi_only_bodyless_operations_without_async_work() {
    let artifact = artifact();
    let result = Runtime::new(&artifact)
        .execute(OperationCall {
            operation: "GET /health",
            content_type: None,
            body: b"",
        })
        .unwrap_or_else(|error| panic!("{error}"));

    assert!(result.request_body().is_none());
    assert_eq!(result.response().status(), 204);
    assert!(result.response().bytes().is_empty());
    assert!(result.async_operations().is_empty());
}

#[test]
fn keeps_scenario_response_controls_out_of_the_operation_call() {
    let artifact = artifact();
    let result = Runtime::new(&artifact)
        .execute_with_options(
            OperationCall {
                operation: "POST /orders",
                content_type: Some("application/json"),
                body: br#"{"product":"beat_pack","quantity":2}"#,
            },
            ExecutionOptions {
                response_status: Some(202),
                response_content_type: None,
                replay: None,
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(result.response().status(), 202);
    assert!(result.response().bytes().is_empty());
}

#[test]
fn fresh_invocations_generate_different_responses() {
    let artifact = artifact();
    let runtime = Runtime::new(&artifact);
    let first = runtime
        .execute(order_call(br#"{"product":"beat_pack","quantity":2}"#))
        .unwrap_or_else(|error| panic!("{error}"));
    let second = runtime
        .execute(order_call(br#"{"product":"beat_pack","quantity":2}"#))
        .unwrap_or_else(|error| panic!("{error}"));

    assert_ne!(first.response().value(), second.response().value());
}

#[test]
fn idempotency_keys_replay_the_first_outcome() {
    let artifact = artifact();
    let runtime = Runtime::new(&artifact);
    let options = ExecutionOptions {
        replay: Some(ReplayIdentity::IdempotencyKey("checkout-42")),
        ..ExecutionOptions::default()
    };
    let first = runtime
        .execute_with_options(
            order_call(br#"{"product":"beat_pack","quantity":2}"#),
            options,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let second = runtime
        .execute_with_options(
            order_call(br#"{"product":"beat_pack","quantity":2}"#),
            options,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let third = runtime
        .execute_with_options(
            order_call(br#"{"product":"beat_pack","quantity":2}"#),
            ExecutionOptions {
                replay: Some(ReplayIdentity::IdempotencyKey("checkout-43")),
                ..ExecutionOptions::default()
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(first, second);
    assert_ne!(first.response().value(), third.response().value());
}

#[test]
fn rejects_reusing_an_idempotency_key_with_a_different_request() {
    let artifact = artifact();
    let runtime = Runtime::new(&artifact);
    let options = ExecutionOptions {
        replay: Some(ReplayIdentity::IdempotencyKey("checkout-42")),
        ..ExecutionOptions::default()
    };
    runtime
        .execute_with_options(
            order_call(br#"{"product":"beat_pack","quantity":2}"#),
            options,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let result = runtime.execute_with_options(
        order_call(br#"{"product":"custom_jingle","quantity":1}"#),
        options,
    );

    assert!(matches!(result, Err(Error::IdempotencyConflict { .. })));
}

#[test]
fn get_resource_identities_replay_the_first_outcome() {
    let artifact = artifact();
    let runtime = Runtime::new(&artifact);
    let options = ExecutionOptions {
        replay: Some(ReplayIdentity::Resource("ord_42")),
        ..ExecutionOptions::default()
    };
    let call = OperationCall {
        operation: "GET /orders/{id}",
        content_type: None,
        body: b"",
    };
    let first = runtime
        .execute_with_options(call, options)
        .unwrap_or_else(|error| panic!("{error}"));
    let second = runtime
        .execute_with_options(call, options)
        .unwrap_or_else(|error| panic!("{error}"));
    let third = runtime
        .execute_with_options(
            call,
            ExecutionOptions {
                replay: Some(ReplayIdentity::Resource("ord_43")),
                ..ExecutionOptions::default()
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(first, second);
    assert_ne!(first.response().value(), third.response().value());
}

#[test]
fn rejects_resource_replay_for_mutating_operations() {
    let artifact = artifact();
    let result = Runtime::new(&artifact).execute_with_options(
        order_call(br#"{"product":"beat_pack","quantity":2}"#),
        ExecutionOptions {
            replay: Some(ReplayIdentity::Resource("ord_42")),
            ..ExecutionOptions::default()
        },
    );

    assert!(matches!(
        result,
        Err(Error::ResourceReplayRequiresGet { .. })
    ));
}

fn order_call(body: &[u8]) -> OperationCall<'_> {
    OperationCall {
        operation: "POST /orders",
        content_type: Some("application/json"),
        body,
    }
}

#[test]
fn runs_a_decoded_form_value_without_coupling_runtime_to_http_encoding() {
    let artifact = artifact();
    let body = json!({"email": "ama@example.com", "name": "Ama Asante"});
    let result = Runtime::new(&artifact)
        .execute_value(OperationValueCall {
            operation: "POST /customers",
            content_type: Some("application/x-www-form-urlencoded"),
            body: Some(&body),
        })
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(
        result.request_body().map(|body| body.media_type()),
        Some("application/x-www-form-urlencoded")
    );
    assert_eq!(
        result.response().value().and_then(|body| body.get("email")),
        Some(&json!("ama@example.com"))
    );
    assert_eq!(
        result.response().value().and_then(|body| body.get("name")),
        Some(&json!("Ama Asante"))
    );
}

fn artifact() -> Artifact {
    let openapi = r#"
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
              type: object
              additionalProperties: false
              required: [product, quantity]
              properties:
                product: { type: string, minLength: 1 }
                quantity: { type: integer, minimum: 1 }
      responses:
        "201":
          description: Created
          content:
            application/json:
              schema:
                type: object
                additionalProperties: false
                required: [id, product, quantity]
                properties:
                  id: { type: string, minLength: 12 }
                  product: { type: string }
                  quantity: { type: integer }
        "202": { description: Accepted }
  /orders/{id}:
    get:
      responses:
        "200":
          description: Order
          content:
            application/json:
              schema:
                type: object
                additionalProperties: false
                required: [id, object]
                properties:
                  id: { type: string, minLength: 12 }
                  object: { type: string, const: order }
  /health:
    get:
      responses:
        "204": { description: Healthy }
  /customers:
    post:
      requestBody:
        required: false
        content:
          application/x-www-form-urlencoded:
            schema:
              type: object
              additionalProperties: false
              properties:
                email: { type: string }
                name: { type: string }
      responses:
        "200":
          description: Customer
          content:
            application/json:
              schema:
                type: object
                additionalProperties: false
                required: [id, object]
                properties:
                  id: { type: string, const: cus_simile }
                  object: { type: string, const: customer }
                  email: { type: string }
                  name: { type: string }
"#;
    let behavior = r#"
resource "db" "orders" {}
resource "mail" "receipts" {}

async "record_order" {
  uses = ["db.orders"]
}

async "send_receipt" {
  uses = ["mail.receipts"]
}

behavior "createOrder" {
  response { status = 201 }
  trigger "record_order" {}
  trigger "send_receipt" {}
}
"#;
    let service = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("orders.yaml", openapi),
            behaviors: vec![Source::new("orders.hcl", behavior)],
        })
        .unwrap_or_else(|error| panic!("{error}"));
    decode(&encode(&service).unwrap_or_else(|error| panic!("{error}")))
        .unwrap_or_else(|error| panic!("{error}"))
}
