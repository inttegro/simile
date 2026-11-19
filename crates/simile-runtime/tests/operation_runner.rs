use serde_json::json;
use simile_artifact::{Artifact, decode, encode};
use simile_compiler::{CompileRequest, Compiler, Source};
use simile_runtime::{
    Error, ExecutionOptions, OperationCall, OperationValueCall, ReplayIdentity, Runtime,
    TransportPlan,
};

#[test]
fn runs_an_operation_through_validation_and_response_synthesis() {
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
fn runs_openapi_only_bodyless_operations() {
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
                outcome: None,
                scenario: None,
                fault_profile: None,
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
fn selects_provider_outcomes_by_name_scenario_and_public_request_predicate() {
    let artifact = artifact();
    assert_eq!(artifact.summary().scenario_selections, 1);
    let runtime = Runtime::new(&artifact);
    let call = order_call(br#"{"product":"beat_pack","quantity":2}"#);

    let named = runtime
        .execute_with_options(
            call,
            ExecutionOptions {
                outcome: Some("card_declined"),
                ..ExecutionOptions::default()
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(named.selected_outcome(), Some("card_declined"));
    assert_eq!(named.response().status(), 402);
    assert_eq!(
        named.response().value(),
        Some(&json!({
            "error": {
                "code": "card_declined",
                "message": "The payment method was declined"
            }
        }))
    );

    let scenario = runtime
        .execute_with_options(
            call,
            ExecutionOptions {
                scenario: Some("payment_declined"),
                ..ExecutionOptions::default()
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(scenario.selected_outcome(), Some("card_declined"));
    assert_eq!(scenario.response().value(), named.response().value());

    let conditional = runtime
        .execute(order_call(br#"{"product":"beat_pack","quantity":99}"#))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(conditional.selected_outcome(), Some("manual_review"));
    assert_eq!(conditional.response().status(), 202);
}

#[test]
fn maps_runtime_failures_through_provider_protocol_errors() {
    let artifact = artifact();
    let response = Runtime::new(&artifact)
        .synthesize_protocol_error("POST /orders", "invalid_body")
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("protocol error should be mapped"));

    assert_eq!(response.status(), 422);
    assert_eq!(
        response.value(),
        Some(&json!({
            "error": {
                "code": "invalid_body",
                "message": "The request body is invalid"
            }
        }))
    );
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

#[test]
fn scenario_faults_change_delivery_without_becoming_service_outcomes() {
    let artifact = artifact();
    assert_eq!(artifact.summary().scenario_faults, 1);
    let runtime = Runtime::new(&artifact);
    let options = ExecutionOptions {
        scenario: Some("response_lost"),
        replay: Some(ReplayIdentity::IdempotencyKey("checkout-lost")),
        ..ExecutionOptions::default()
    };

    let first = runtime
        .execute_with_options(
            order_call(br#"{"product":"beat_pack","quantity":2}"#),
            options,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(first.selected_outcome(), None);
    assert_eq!(
        first.fault().map(|fault| fault.name()),
        Some("response_lost")
    );
    assert_eq!(first.transport(), &TransportPlan::DisconnectAfterCommit);

    let replay = runtime
        .execute_with_options(
            order_call(br#"{"product":"beat_pack","quantity":2}"#),
            options,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(replay.fault(), None);
    assert_eq!(replay.transport(), &TransportPlan::Respond);
    assert_eq!(replay.response(), first.response());
}

#[test]
fn stochastic_fault_profiles_are_seeded_scoped_and_bounded() {
    let artifact = artifact();
    let runtime = Runtime::new(&artifact);
    assert_eq!(
        runtime
            .fault_profile_seed("always_slow")
            .unwrap_or_else(|error| panic!("{error}")),
        7
    );
    let options = ExecutionOptions {
        fault_profile: Some("always_slow"),
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
            options,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let fourth = runtime
        .execute_with_options(
            order_call(br#"{"product":"beat_pack","quantity":2}"#),
            options,
        )
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(
        first.transport(),
        &TransportPlan::RespondAfter { milliseconds: 25 }
    );
    assert_eq!(second.transport(), &TransportPlan::Respond);
    assert_eq!(third.transport(), &TransportPlan::Respond);
    assert_eq!(
        fourth.transport(),
        &TransportPlan::RespondAfter { milliseconds: 25 }
    );
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
        "402":
          description: Payment required
          content:
            application/json:
              schema:
                type: object
                additionalProperties: false
                required: [error]
                properties:
                  error:
                    type: object
                    additionalProperties: false
                    required: [code, message]
                    properties:
                      code: { type: string }
                      message: { type: string }
        "422":
          description: Invalid body
          content:
            application/json:
              schema:
                type: object
                additionalProperties: false
                required: [error]
                properties:
                  error:
                    type: object
                    additionalProperties: false
                    required: [code, message]
                    properties:
                      code: { type: string }
                      message: { type: string }
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

error_set "payment_errors" {
  outcome "card_declined" {
    status       = 402
    content_type = "application/json"
    body = {
      error = {
        code    = "card_declined"
        message = "The payment method was declined"
      }
    }
  }
}

protocol_error "invalid_body" {
  statuses     = [422, 400]
  content_type = "application/json"
  body = {
    error = {
      code    = "invalid_body"
      message = "The request body is invalid"
    }
  }
}

scenario "payment_declined" {
  select "createOrder" { outcome = "card_declined" }
}

fault "slow_response" {
  phase = "before_response"
  delay { milliseconds = 25 }
}

fault "response_lost" {
  phase = "after_commit"
  disconnect {}
}

fault_profile "always_slow" {
  probability      = 1
  faults           = ["slow_response"]
  operations       = ["createOrder"]
  max_consecutive  = 1
  cooldown_requests = 2
  seed             = 7
}

scenario "response_lost" {
  inject "createOrder" { fault = "response_lost" }
}

behavior "createOrder" {
  error_sets = ["payment_errors"]
  response { status = 201 }
  outcome "manual_review" {
    status = 202
    when {
      path   = "/quantity"
      equals = 99
    }
  }
}
"#;
    let service = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("orders.yaml", openapi),
            behaviors: vec![Source::new("orders.hcl", behavior)],
            heuristic_policy: simile_heuristics::Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    decode(&encode(&service).unwrap_or_else(|error| panic!("{error}")))
        .unwrap_or_else(|error| panic!("{error}"))
}
