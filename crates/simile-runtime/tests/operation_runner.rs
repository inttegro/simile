use serde_json::json;
use simile_artifact::{Artifact, decode, encode};
use simile_compiler::{CompileRequest, Compiler, Source};
use simile_runtime::{Error, ExecutionOptions, OperationCall, Runtime};

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
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(result.response().status(), 202);
    assert!(result.response().bytes().is_empty());
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
                  id: { type: string, const: ord_simile }
                  product: { type: string }
                  quantity: { type: integer }
        "202": { description: Accepted }
  /health:
    get:
      responses:
        "204": { description: Healthy }
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
