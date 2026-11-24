use std::collections::BTreeMap;

use serde_json::json;
use simile_artifact::{Artifact, decode, encode};
use simile_compiler::{CompileRequest, Compiler, Source};
use simile_runtime::{
    Error, ExecutionOptions, IncomingParameters, OperationCall, OperationValueCall, ReplayIdentity,
    Runtime, TransportPlan, authenticate, authenticate_with_profiles,
};
use simile_spec::{Profile, ProfileCredential, ProfileStatus};

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
fn anchors_server_owned_timestamps_to_the_logical_operation_time() {
    let artifact = timestamp_artifact();
    let logical_time_milliseconds = 1_700_000_000_123;
    let expected = json!("2023-11-14T22:13:20.123Z");

    let create = Runtime::new(&artifact)
        .execute_with_options(
            OperationCall {
                operation: "POST /prices/create",
                content_type: None,
                body: &[],
            },
            ExecutionOptions {
                logical_time_milliseconds: Some(logical_time_milliseconds),
                ..ExecutionOptions::default()
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        create
            .response()
            .value()
            .and_then(|body| body.pointer("/price/created_at")),
        Some(&expected)
    );
    assert_eq!(
        create
            .response()
            .value()
            .and_then(|body| body.pointer("/price/created")),
        Some(&json!(1_700_000_000))
    );
    assert!(
        create
            .response()
            .value()
            .and_then(|body| body.pointer("/price/updated_at"))
            .is_none()
    );

    let update = Runtime::new(&artifact)
        .execute_with_options(
            OperationCall {
                operation: "POST /prices/update",
                content_type: None,
                body: &[],
            },
            ExecutionOptions {
                logical_time_milliseconds: Some(logical_time_milliseconds),
                ..ExecutionOptions::default()
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        update
            .response()
            .value()
            .and_then(|body| body.pointer("/price/updated_at")),
        Some(&expected)
    );

    let archive = Runtime::new(&artifact)
        .execute_with_options(
            OperationCall {
                operation: "POST /prices/archive",
                content_type: None,
                body: &[],
            },
            ExecutionOptions {
                logical_time_milliseconds: Some(logical_time_milliseconds),
                ..ExecutionOptions::default()
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        archive
            .response()
            .value()
            .and_then(|body| body.pointer("/price/archived_at")),
        Some(&expected)
    );
    assert_eq!(
        archive
            .response()
            .value()
            .and_then(|body| body.pointer("/price/updated_at")),
        Some(&expected)
    );
}

#[test]
fn explicit_request_timestamp_bindings_override_the_service_clock() {
    let artifact = timestamp_artifact();
    let result = Runtime::new(&artifact)
        .execute_with_options(
            OperationCall {
                operation: "POST /events/create",
                content_type: Some("application/json"),
                body: br#"{"created_at":"2020-01-02T03:04:05Z"}"#,
            },
            ExecutionOptions {
                logical_time_milliseconds: Some(1_700_000_000_123),
                ..ExecutionOptions::default()
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        result
            .response()
            .value()
            .and_then(|body| body.pointer("/created_at")),
        Some(&json!("2020-01-02T03:04:05Z"))
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
fn plans_committed_effects_from_validated_request_and_response_values() {
    let artifact = artifact();
    let result = Runtime::new(&artifact)
        .execute(order_call(br#"{"product":"beat_pack","quantity":2}"#))
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(result.effects().synchronous().len(), 1);
    let action = &result.effects().synchronous()[0];
    assert_eq!(action.resource().as_str(), "db.orders");
    assert_eq!(action.kind(), simile_spec::EffectActionKind::DbPut);
    assert_eq!(action.arguments()["collection"], json!("orders"));
    assert_eq!(
        action.arguments()["key"],
        result.response().value().unwrap_or_else(|| panic!("body"))["id"]
    );
    assert_eq!(
        action.arguments()["value"],
        result
            .response()
            .value()
            .cloned()
            .unwrap_or_else(|| panic!("body"))
    );
}

#[test]
fn plans_schema_checked_webhook_occurrences_from_effects() {
    let artifact = artifact();
    let result = Runtime::new(&artifact)
        .execute(order_call(br#"{"product":"beat_pack","quantity":2}"#))
        .unwrap_or_else(|error| panic!("{error}"));

    let webhook = result
        .effects()
        .webhooks()
        .first()
        .unwrap_or_else(|| panic!("webhook missing"));
    assert_eq!(webhook.name(), "order.created");
    assert_eq!(webhook.method(), "POST");
    assert_eq!(webhook.content_type(), Some("application/json"));
    assert_eq!(webhook.target(), Some("https://merchant.test/hooks"));
    assert_eq!(
        webhook.body(),
        result
            .response()
            .value()
            .unwrap_or_else(|| panic!("response body missing"))
    );
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
                response_seed: None,
                response_body: None,
                response_request_body: None,
                parameters: None,
                logical_time_milliseconds: None,
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

    assert_eq!(first.outcome(), second.outcome());
    assert_eq!(first.fault(), second.fault());
    assert_eq!(first.transport(), second.transport());
    assert!(!first.effects().is_empty());
    assert!(second.effects().is_empty());
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

#[test]
fn form_decoding_uses_the_schema_without_corrupting_numeric_strings() {
    let artifact = artifact();
    let result = Runtime::new(&artifact)
        .execute(OperationCall {
            operation: "POST /customers",
            content_type: Some("application/x-www-form-urlencoded"),
            body: b"postal_code=00123&quantity=2",
        })
        .unwrap_or_else(|error| panic!("{error}"));
    let request = result
        .request_body()
        .map(simile_runtime::ValidatedBody::value)
        .unwrap_or_else(|| panic!("missing decoded form"));

    assert_eq!(request.get("postal_code"), Some(&json!("00123")));
    assert_eq!(request.get("quantity"), Some(&json!(2)));
}

#[test]
fn authenticates_test_principals_and_enforces_oauth_scopes() {
    let artifact = authentication_artifact();
    let mut incoming = IncomingParameters::default();
    incoming.headers.insert(
        "authorization".to_owned(),
        vec!["Bearer sk_test_merchant".to_owned()],
    );
    let principal = authenticate(&artifact, "GET /orders", &incoming)
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("principal missing"));
    assert_eq!(principal.name(), "merchant_app");
    assert!(!principal.is_dynamic());

    incoming.headers.insert(
        "authorization".to_owned(),
        vec!["Bearer sk_test_readonly".to_owned()],
    );
    assert!(matches!(
        authenticate(&artifact, "GET /orders", &incoming),
        Err(Error::PermissionDenied { .. })
    ));
}

#[test]
fn authenticates_runtime_issued_profile_principals_without_a_naming_convention() {
    let artifact = authentication_artifact();
    let profile = Profile {
        name: "production-like".to_owned(),
        principal: "merchant_application".to_owned(),
        status: ProfileStatus::Active,
        credentials: BTreeMap::from([(
            "merchantOAuth".to_owned(),
            ProfileCredential::Secret {
                value: "sk_profile_issued".to_owned(),
            },
        )]),
    };
    let mut incoming = IncomingParameters::default();
    incoming.headers.insert(
        "authorization".to_owned(),
        vec!["Bearer sk_profile_issued".to_owned()],
    );

    let principal = authenticate_with_profiles(
        &artifact,
        "GET /orders",
        &incoming,
        std::slice::from_ref(&profile),
    )
    .unwrap_or_else(|error| panic!("{error}"))
    .unwrap_or_else(|| panic!("principal missing"));

    assert_eq!(principal.name(), "production-like");
    assert_eq!(principal.principal(), "merchant_application");
    assert!(principal.is_dynamic());
}

#[test]
fn rejects_revoked_and_unknown_credentials() {
    let artifact = authentication_artifact();
    for token in ["sk_test_revoked", "sk_test_unknown"] {
        let mut incoming = IncomingParameters::default();
        incoming
            .headers
            .insert("authorization".to_owned(), vec![format!("Bearer {token}")]);
        assert!(matches!(
            authenticate(&artifact, "GET /orders", &incoming),
            Err(Error::AuthenticationRequired { .. })
        ));
    }
}

#[test]
fn treats_an_empty_security_alternative_as_public_access() {
    let artifact = authentication_artifact();
    let principal = authenticate(&artifact, "GET /optional", &IncomingParameters::default())
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(principal.is_none());
}

#[test]
fn optional_authentication_prefers_a_valid_presented_profile() {
    let artifact = authentication_artifact();
    let mut incoming = IncomingParameters::default();
    incoming.headers.insert(
        "authorization".to_owned(),
        vec!["Bearer sk_test_merchant".to_owned()],
    );

    // Why: an optional-auth operation can expose a richer caller-scoped view; silently selecting
    // the anonymous alternative would make valid authentication ineffective.
    let principal = authenticate(&artifact, "GET /optional", &incoming)
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("valid optional authentication was ignored"));
    assert_eq!(principal.name(), "merchant_app");
}

fn authentication_artifact() -> Artifact {
    let openapi = r#"
openapi: 3.1.2
info: { title: Authenticated orders, version: 1.0.0 }
components:
  securitySchemes:
    merchantOAuth:
      type: oauth2
      flows:
        clientCredentials:
          tokenUrl: https://example.test/oauth/token
          scopes:
            orders:read: Read orders
paths:
  /orders:
    get:
      security:
        - merchantOAuth: [orders:read]
      responses:
        "200": { description: Orders }
  /optional:
    get:
      security:
        - {}
        - merchantOAuth: [orders:read]
      responses:
        "200": { description: Public or authenticated }
"#;
    let behavior = r#"
principal "merchant_application" {
  description = "A merchant application with order access."
  credential "merchantOAuth" { grants = ["orders:read"] }
}

principal "readonly_application" {
  credential "merchantOAuth" {}
}

profile "merchant_app" {
  principal = principal.merchant_application
  credential "merchantOAuth" { value = "sk_test_merchant" }
}

profile "readonly_app" {
  principal = principal.readonly_application
  credential "merchantOAuth" { value = "sk_test_readonly" }
}

profile "revoked_app" {
  principal = principal.merchant_application
  status = "revoked"
  credential "merchantOAuth" { value = "sk_test_revoked" }
}
"#;
    let service = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("auth.yaml", openapi),
            modules: vec![Source::new("auth.hcl", behavior)],
            heuristic_policy: simile_heuristics::Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    decode(&encode(&service).unwrap_or_else(|error| panic!("{error}")))
        .unwrap_or_else(|error| panic!("{error}"))
}

fn timestamp_artifact() -> Artifact {
    let openapi = r#"
openapi: 3.1.0
info: { title: Timestamped resources, version: 1.0.0 }
paths:
  /prices/create:
    post:
      operationId: createPrice
      responses:
        '200':
          description: created
          content:
            application/json:
              schema: { $ref: '#/components/schemas/PriceResponse' }
  /prices/update:
    post:
      operationId: updatePrice
      responses:
        '200':
          description: updated
          content:
            application/json:
              schema: { $ref: '#/components/schemas/PriceResponse' }
  /prices/archive:
    post:
      operationId: archivePrice
      responses:
        '200':
          description: archived
          content:
            application/json:
              schema: { $ref: '#/components/schemas/PriceResponse' }
  /events/create:
    post:
      operationId: createEvent
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              required: [created_at]
              properties:
                created_at: { type: string, format: date-time }
      responses:
        '200':
          description: created
          content:
            application/json:
              schema:
                type: object
                required: [created_at]
                properties:
                  created_at: { type: string, format: date-time }
              x-simile-from-request:
                bindings:
                  - { from: /created_at, to: /created_at }
components:
  schemas:
    PriceResponse:
      type: object
      additionalProperties: false
      required: [price]
      properties:
        price:
          type: object
          additionalProperties: false
          required: [id, created, created_at]
          properties:
            id: { type: string, const: pr_clock }
            created: { type: integer, format: unix-time }
            created_at: { type: string, format: date-time }
            updated_at: { type: string, format: date-time }
            archived_at: { type: string, format: date-time }
"#;
    let service = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("timestamps.yaml", openapi),
            modules: Vec::new(),
            heuristic_policy: simile_heuristics::Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    decode(&encode(&service).unwrap_or_else(|error| panic!("{error}")))
        .unwrap_or_else(|error| panic!("{error}"))
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
                postal_code: { type: string }
                quantity: { type: integer }
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
webhooks:
  order.created:
    post:
      requestBody:
        required: true
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
      responses:
        "204": { description: Accepted }
"#;
    let behavior = r#"
backend "db" "orders" {}
backend "mail" "receipts" {}

async "record_order" {
  uses = ["db.orders"]
}

effect "persist_order" {
  operations = ["createOrder"]
  emit = {
    "order.created" = response
  }
  to = {
    "order.created" = "https://merchant.test/hooks"
  }
  action "db.orders" "put" {
    collection = "orders"
    key        = response.id
    value      = response
  }
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
            modules: vec![Source::new("orders.hcl", behavior)],
            heuristic_policy: simile_heuristics::Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    decode(&encode(&service).unwrap_or_else(|error| panic!("{error}")))
        .unwrap_or_else(|error| panic!("{error}"))
}
