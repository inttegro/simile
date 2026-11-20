use simile_hcl::Source;
use simile_spec::{ProfileCredential, ProfileStatus, ResourceKind, ResourceRef};

#[test]
fn compiles_service_setup_into_a_profile_contract() {
    let document = simile_hcl::parse_service(Source::new(
        "service.hcl",
        r#"
service "orders" {
  listen { port = 4242 }
  openapi { path = "./orders.yaml" }
}

principal "application" {
  credential "merchantOAuth" {
    grants = ["orders:write"]
  }
}

principal_setup "application" {
  description = "Create one application."
  operation   = "createOrder"

  collect "name" {
    required = true
    to       = request.name
  }

  issue "access_token" {
    generate  = "token"
    prefix    = "tok_"
    credential = "merchantOAuth"
    sensitive = true
  }
}
"#,
    ))
    .unwrap_or_else(|error| panic!("{error}"));
    let principals = simile_hcl::compile(
        [module(
            r#"
principal "application" {
  credential "merchantOAuth" {
    grants = ["orders:write"]
  }
}
"#,
        )],
        &secured_contract(),
    )
    .unwrap_or_else(|error| panic!("{error}"))
    .principals;
    let setups = simile_hcl::compile_principal_setups(
        document.principal_setups,
        &secured_contract(),
        &principals,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let setup = &setups["application"];

    assert_eq!(setup.principal, "application");
    assert_eq!(setup.operation_id, "createOrder");
    assert_eq!(setup.collects["name"].request_pointer, "/name");
    assert_eq!(
        setup.issues["access_token"].credential.as_deref(),
        Some("merchantOAuth")
    );
}

#[test]
fn compiles_setup_workflows_for_multiple_principal_kinds() {
    let document = simile_hcl::parse_service(Source::new(
        "service.hcl",
        r#"
service "orders" {
  listen { port = 4242 }
  openapi { path = "./orders.yaml" }
}

principal "application" {
  credential "merchantOAuth" {}
}

principal "operator" {
  credential "merchantKey" {}
}

principal_setup "application" {
  operation = "createOrder"
  issue "access_token" {
    generate   = "token"
    credential = "merchantOAuth"
  }
}

principal_setup "operator" {
  operation = "createOrder"
  issue "api_key" {
    generate   = "token"
    credential = "merchantKey"
  }
}
"#,
    ))
    .unwrap_or_else(|error| panic!("{error}"));
    let principal_source = module(
        r#"
principal "application" {
  credential "merchantOAuth" {}
}

principal "operator" {
  credential "merchantKey" {}
}
"#,
    );
    let principals = simile_hcl::compile([principal_source], &secured_contract())
        .unwrap_or_else(|error| panic!("{error}"))
        .principals;
    let setups = simile_hcl::compile_principal_setups(
        document.principal_setups,
        &secured_contract(),
        &principals,
    )
    .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(setups.len(), 2);
    assert_eq!(setups["application"].principal, "application");
    assert_eq!(setups["operator"].principal, "operator");
}

#[test]
fn compiles_local_test_principals_against_openapi_security_schemes() {
    let compiled = simile_hcl::compile(
        [module(
            r#"
principal "application" {
  description = "A merchant integration with order write access."
  credential "merchantOAuth" {
    grants = ["orders:write"]
  }
  credential "merchantKey" {
  }
}

profile "merchant_app" {
  principal = principal.application
  credential "merchantOAuth" { value = "sim_test_merchant" }
}

profile "revoked_key" {
  principal = principal.application
  status = "revoked"
  credential "merchantKey" { value = "revoked_test_key" }
}
"#,
        )],
        &secured_contract(),
    )
    .unwrap_or_else(|error| panic!("{error}"));

    let application = &compiled.principals["application"];
    assert!(
        application.credentials["merchantOAuth"]
            .grants
            .contains("orders:write")
    );
    let merchant = &compiled.profiles["merchant_app"];
    assert_eq!(merchant.status, ProfileStatus::Active);
    assert_eq!(merchant.principal, "application");
    assert_eq!(
        merchant.credentials["merchantOAuth"],
        ProfileCredential::Secret {
            value: "sim_test_merchant".to_owned()
        }
    );
    assert_eq!(
        compiled.profiles["revoked_key"].status,
        ProfileStatus::Revoked
    );
}

#[test]
fn parses_and_compiles_behavior_modules() {
    let api = contract();
    let module = simile_hcl::parse(Source::new(
        "orders.hcl",
        r#"
backend "db" "orders" {}
backend "event_bus" "commerce" {}

async "publish_order" {
  uses = ["db.orders", "event_bus.commerce"]
}

behavior "createOrder" {
  response {
    status = 201
  }
}
"#,
    ))
    .unwrap_or_else(|error| panic!("{error}"));

    let compiled = simile_hcl::compile([module], &api).unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(compiled.resources.len(), 2);
    assert_eq!(compiled.behaviors["createOrder"].response_status, Some(201));
    assert_eq!(
        compiled.async_operations["publish_order"].uses,
        [
            ResourceRef::new(ResourceKind::Db, "orders"),
            ResourceRef::new(ResourceKind::EventBus, "commerce"),
        ]
    );
}

#[test]
fn rejects_provider_specific_backends() {
    let error = parse_error(r#"backend "postgres" "orders" {}"#);

    assert!(error.to_string().contains("not a Simile abstraction"));
}

#[test]
fn rejects_behavior_for_an_unknown_operation() {
    let module = module(
        r#"
behavior "secretProductionCheck" {
  response { status = 200 }
}
"#,
    );

    let error = compile_error([module]);

    assert!(error.to_string().contains("unknown OpenAPI operationId"));
}

#[test]
fn rejects_a_response_absent_from_openapi() {
    let module = module(
        r#"
behavior "createOrder" {
  response { status = 202 }
}
"#,
    );

    let error = compile_error([module]);

    assert!(
        error
            .to_string()
            .contains("does not declare response status 202")
    );
}

#[test]
fn rejects_unknown_backend_references() {
    let operation = module(
        r#"
async "publish_order" {
  uses = ["queue.missing"]
}
"#,
    );
    let error = compile_error([operation]);
    assert!(error.to_string().contains("undeclared backend"));
}

#[test]
fn compiles_typed_effects_and_public_value_projections() {
    let compiled = simile_hcl::compile(
        [module(
            r#"
backend "db" "orders" {}
backend "queue" "fulfillment" {}

async "publish_order" {
  action "queue.fulfillment" "publish" {
    body = {
      id       = response.id
      customer = request.customer_id
    }
  }
}

effect "record_order" {
  description = "Persist the accepted order and publish it for fulfillment."
  operations  = ["createOrder"]
  async       = ["publish_order"]

  action "db.orders" "put" {
    collection = "orders"
    key        = response.id
    value      = response
  }
}
"#,
        )],
        &contract(),
    )
    .unwrap_or_else(|error| panic!("{error}"));

    let effect = &compiled.effects["record_order"];
    assert_eq!(effect.operations, ["createOrder"]);
    assert_eq!(effect.async_operations, ["publish_order"]);
    assert_eq!(effect.actions.len(), 1);
    assert_eq!(effect.actions[0].kind, simile_spec::EffectActionKind::DbPut);
    assert_eq!(
        effect.actions[0].arguments["key"],
        simile_spec::EffectValue::Response {
            pointer: "/id".to_owned()
        }
    );
    assert_eq!(
        compiled.async_operations["publish_order"].uses,
        [ResourceRef::new(ResourceKind::Queue, "fulfillment")]
    );
    assert_eq!(
        compiled
            .descriptions
            .find(simile_spec::DescriptionKind::Effect, None, "record_order"),
        Some("Persist the accepted order and publish it for fulfillment.")
    );
}

#[test]
fn rejects_invalid_effect_actions_before_runtime() {
    let unsupported = parse_error(
        r#"
effect "read_from_db" {
  operations = ["createOrder"]
  action "db.orders" "get" { key = response.id }
}
"#,
    );
    assert!(unsupported.to_string().contains("not an effect action"));

    let missing = parse_error(
        r#"
effect "incomplete_write" {
  operations = ["createOrder"]
  action "db.orders" "put" {
    collection = "orders"
    key        = response.id
  }
}
"#,
    );
    assert!(missing.to_string().contains("requires argument `value`"));
}

#[test]
fn rejects_removed_behavior_trigger_syntax() {
    let error = parse_error(
        r#"
behavior "createOrder" {
  trigger "publish_order" {}
}
"#,
    );

    assert!(
        error
            .to_string()
            .contains("`behavior` does not accept nested block `trigger`")
    );
}

#[test]
fn resolves_cross_module_references_without_file_ordering() {
    let operation = module(
        r#"
async "publish_order" {
  uses = ["queue.orders"]
}
"#,
    );
    let resources = module(
        r#"
backend "queue" "orders" {}
"#,
    );

    let compiled = simile_hcl::compile([operation, resources], &contract())
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(compiled.async_operations.len(), 1);
    assert_eq!(compiled.resources.len(), 1);
}

#[test]
fn compiles_environmental_faults_separately_from_error_outcomes() {
    let compiled = simile_hcl::compile(
        [module(
            r#"
fault "slow_response" {
  phase = "before_response"
  delay { milliseconds = 250 }
}

fault "response_lost" {
  phase = "after_commit"
  disconnect {}
}

fault_profile "rough_network" {
  probability       = 0.02
  faults            = ["slow_response", "response_lost"]
  operations        = ["createOrder"]
  max_consecutive   = 1
  cooldown_requests = 20
  seed              = 42
}

scenario "lost_after_commit" {
  inject "createOrder" { fault = "response_lost" }
}
"#,
        )],
        &contract(),
    )
    .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(compiled.faults.len(), 2);
    assert_eq!(
        compiled.fault_profiles["rough_network"].probability_ppm,
        20_000
    );
    assert_eq!(
        compiled.scenarios["lost_after_commit"].faults["createOrder"],
        "response_lost"
    );
}

#[test]
fn imports_catalog_faults_with_defaults_and_typed_overrides() {
    let compiled = simile_hcl::compile(
        [module(
            r#"
faults {
  use "http.slow" {}
  use "http.slow" {
    as      = "very_slow"
    latency = "5s"
  }
  use "http.response_lost" {}
}

fault_profile "rough_network" {
  probability = 0.02
  faults      = ["http.slow", "http.response_lost"]
}

scenario "slow_order" {
  inject "createOrder" { fault = "very_slow" }
}
"#,
        )],
        &contract(),
    )
    .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(
        compiled.faults["http.slow"].action,
        simile_spec::FaultAction::Delay {
            milliseconds: 1_000
        }
    );
    assert_eq!(
        compiled.faults["very_slow"].action,
        simile_spec::FaultAction::Delay {
            milliseconds: 5_000
        }
    );
    assert_eq!(
        compiled.faults["http.response_lost"].action,
        simile_spec::FaultAction::Disconnect
    );
    assert_eq!(
        compiled.scenarios["slow_order"].faults["createOrder"],
        "very_slow"
    );
    assert_eq!(
        compiled
            .descriptions
            .find(simile_spec::DescriptionKind::Fault, None, "http.slow"),
        Some("Delay the completed response before delivery.")
    );
    assert_eq!(
        compiled
            .descriptions
            .find(simile_spec::DescriptionKind::Fault, None, "very_slow"),
        Some("Delay the completed response before delivery.")
    );
}

#[test]
fn retains_public_descriptions_without_changing_behavior() {
    let compiled = simile_hcl::compile(
        [module(
            r#"
backend "queue" "orders" {
  description = "Carries accepted orders."
}

async "dispatch_order" {
  description = "Queue the order for fulfillment."
  uses = ["queue.orders"]
}

behavior "createOrder" {
  description = "Create one order."
  outcome "accepted" {
    description = "The order was accepted."
    status = 201
  }
}

scenario "accepted_order" {
  description = "Force the accepted result."
  select "createOrder" { outcome = "accepted" }
}
"#,
        )],
        &contract(),
    )
    .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(
        compiled
            .descriptions
            .find(simile_spec::DescriptionKind::Behavior, None, "createOrder"),
        Some("Create one order.")
    );
    assert_eq!(
        compiled.descriptions.find(
            simile_spec::DescriptionKind::Outcome,
            Some("behavior:createOrder"),
            "accepted"
        ),
        Some("The order was accepted.")
    );
    assert_eq!(compiled.behaviors["createOrder"].outcomes[0].status, 201);
}

#[test]
fn rejects_unknown_or_incompatible_catalog_fault_configuration() {
    let unknown = compile_error([module(
        r#"
faults {
  use "http.not_real" {}
}
"#,
    )]);
    assert!(unknown.to_string().contains("does not exist"));
    assert!(unknown.to_string().contains("http.slow"));

    let incompatible = compile_error([module(
        r#"
faults {
  use "http.response_lost" {
    latency = "2s"
  }
}
"#,
    )]);
    assert!(
        incompatible
            .to_string()
            .contains("does not support the `latency` override")
    );

    let invalid_duration = parse_error(
        r#"
faults {
  use "http.slow" {
    latency = "2 fortnights"
  }
}
"#,
    );
    assert!(invalid_duration.to_string().contains("duration from 1ms"));
}

#[test]
fn composes_catalog_fault_sets_with_wildcards_and_exclusions() {
    let all_except_lost = simile_hcl::compile(
        [module(
            r#"
faults {
  use "*" {}
  use "^http.response_lost" {}
}
"#,
        )],
        &contract(),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        all_except_lost
            .faults
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["http.slow"]
    );

    let namespace_union = simile_hcl::compile(
        [
            module(
                r#"
faults {
  use "http.*" {}
  use "http.slow" {}
}
"#,
            ),
            module(
                r#"
faults {
  use "^http.response_lost" {}
}
"#,
            ),
        ],
        &contract(),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(namespace_union.faults.len(), 1);
    assert!(namespace_union.faults.contains_key("http.slow"));

    let namespace_exclusion = simile_hcl::compile(
        [module(
            r#"
faults {
  use "*" {}
  use "^http.*" {}
}
"#,
        )],
        &contract(),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert!(namespace_exclusion.faults.is_empty());
}

#[test]
fn rejects_ambiguous_or_mistyped_catalog_selectors() {
    let configured_set = compile_error([module(
        r#"
faults {
  use "http.*" {
    latency = "2s"
  }
}
"#,
    )]);
    assert!(
        configured_set
            .to_string()
            .contains("selects multiple faults")
    );

    let unmatched = compile_error([module(
        r#"
faults {
  use "network.*" {}
}
"#,
    )]);
    assert!(unmatched.to_string().contains("does not match any"));

    for invalid in ["http.s*", "^", "http.*.slow", "http*.*"] {
        let source = format!(
            r#"
faults {{
  use "{invalid}" {{}}
}}
"#
        );
        let error = parse_error(&source);
        assert!(
            error.to_string().contains("invalid fault selector"),
            "selector {invalid:?} produced {error}"
        );
    }

    let configured_exclusion = parse_error(
        r#"
faults {
  use "^http.slow" {
    latency = "2s"
  }
}
"#,
    );
    assert!(
        configured_exclusion
            .to_string()
            .contains("cannot contain overrides")
    );
}

#[test]
fn rejects_faults_that_are_unsafe_or_unresolvable() {
    let incompatible = compile_error([module(
        r#"
fault "early_disconnect" {
  phase = "before_response"
  disconnect {}
}
"#,
    )]);
    assert!(incompatible.to_string().contains("incompatible phase"));

    let unknown = compile_error([module(
        r#"
fault_profile "broken" {
  probability = 0.1
  faults      = ["missing"]
}
"#,
    )]);
    assert!(unknown.to_string().contains("unknown fault `missing`"));

    let empty = parse_error(
        r#"
fault_profile "empty" {
  probability = 0.1
  faults      = []
}
"#,
    );
    assert!(empty.to_string().contains("non-empty array of strings"));

    let duplicated = compile_error([module(
        r#"
fault "slow" {
  phase = "before_response"
  delay { milliseconds = 10 }
}

fault_profile "biased_by_accident" {
  probability = 0.1
  faults      = ["slow", "slow"]
}
"#,
    )]);
    assert!(duplicated.to_string().contains("more than once"));

    let ambiguous = compile_error([module(
        r#"
fault "slow" {
  phase = "before_response"
  delay { milliseconds = 10 }
}

scenario "ambiguous" {
  inject "createOrder" { fault = "slow" }
  inject "createOrder" { fault = "slow" }
}
"#,
    )]);
    assert!(ambiguous.to_string().contains("more than once"));
}

#[test]
fn compiles_one_service_wide_webhook_delivery_policy() {
    let spec = simile_hcl::compile(
        [module(
            r#"
webhooks {
  ordering                 = "best_effort"
  out_of_order_probability = 0.125
  reorder_window           = "4s"
  concurrency              = 6

  signing {
    secret           = "whsec_simile_local"
    header           = "X-Service-Signature"
    timestamp_header = "X-Service-Timestamp"
  }
}
"#,
        )],
        &contract(),
    )
    .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(
        spec.webhook_policy,
        simile_spec::WebhookPolicy {
            ordering: simile_spec::WebhookOrdering::BestEffort,
            out_of_order_probability_ppm: 125_000,
            reorder_window_milliseconds: 4_000,
            concurrency: 6,
            signing: Some(simile_spec::WebhookSigning {
                secret: "whsec_simile_local".to_owned(),
                header: "X-Service-Signature".to_owned(),
                timestamp_header: "X-Service-Timestamp".to_owned(),
            }),
        }
    );

    let duplicate = compile_error([
        module("webhooks { ordering = \"strict\" }"),
        module("webhooks { ordering = \"strict\" }"),
    ]);
    assert!(matches!(
        duplicate,
        simile_hcl::Error::DuplicateWebhookPolicy
    ));
}

#[test]
fn defaults_to_realistic_best_effort_webhook_delivery() {
    let spec =
        simile_hcl::compile([module("")], &contract()).unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(spec.webhook_policy, simile_spec::WebhookPolicy::default());
}

#[test]
fn compiles_provider_webhooks_and_compact_effect_emissions() {
    let source = r#"
webhook "payment.succeeded" {
  description = "A payment reached its terminal success state."
}

webhook "order.paid" {}
webhook "receipt.available" {}

effect "announce_payment" {
  operations = ["createOrder"]

  emit = {
    "payment.succeeded" = response.payment
    "order.paid"        = response.order
    "receipt.available" = response.receipt
  }

  after = {
    "order.paid" = ["payment.succeeded"]
  }

  delay = {
    "receipt.available" = "2s"
  }

  to = {
    "payment.succeeded" = request.callback_url
  }
}
"#;
    let spec = simile_hcl::compile([module(source)], &contract())
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(spec.webhooks.len(), 3);
    let effect = &spec.effects["announce_payment"];
    assert_eq!(effect.webhooks.len(), 3);
    let order = effect
        .webhooks
        .iter()
        .find(|emission| emission.webhook == "order.paid")
        .unwrap_or_else(|| panic!("order webhook missing"));
    assert_eq!(order.after, ["payment.succeeded"]);
    let receipt = effect
        .webhooks
        .iter()
        .find(|emission| emission.webhook == "receipt.available")
        .unwrap_or_else(|| panic!("receipt webhook missing"));
    assert_eq!(receipt.delay_milliseconds, 2_000);
}

#[test]
fn rejects_incomplete_or_cyclic_webhook_emissions() {
    let missing_emit = parse_error(
        r#"
effect "announce_payment" {
  operations = ["createOrder"]
  after = { "order.paid" = ["payment.succeeded"] }
}
"#,
    );
    assert!(missing_emit.to_string().contains("attribute `emit`"));

    let cycle = compile_error([module(
        r#"
webhook "payment.succeeded" {}
webhook "order.paid" {}

effect "announce_payment" {
  operations = ["createOrder"]
  emit = {
    "payment.succeeded" = response.payment
    "order.paid"        = response.order
  }
  after = {
    "payment.succeeded" = ["order.paid"]
    "order.paid"        = ["payment.succeeded"]
  }
}
"#,
    )]);
    assert!(cycle.to_string().contains("dependency cycle"));
}

fn module(source: &str) -> simile_hcl::Module {
    simile_hcl::parse(Source::new("test.hcl", source)).unwrap_or_else(|error| panic!("{error}"))
}

fn parse_error(source: &str) -> simile_hcl::Error {
    match simile_hcl::parse(Source::new("test.hcl", source)) {
        Err(error) => error,
        Ok(module) => panic!("expected parsing to fail, parsed {module:?}"),
    }
}

fn compile_error(modules: impl IntoIterator<Item = simile_hcl::Module>) -> simile_hcl::Error {
    match simile_hcl::compile(modules, &contract()) {
        Err(error) => error,
        Ok(spec) => panic!("expected compilation to fail, compiled {spec:?}"),
    }
}

fn contract() -> simile_spec::ApiContract {
    simile_openapi::parse(
        r#"
openapi: 3.1.2
info:
  title: Orders
  version: 1.0.0
paths:
  /orders:
    post:
      operationId: createOrder
      responses:
        "201": { description: Created }
"#,
    )
    .unwrap_or_else(|error| panic!("{error}"))
}

fn secured_contract() -> simile_spec::ApiContract {
    simile_openapi::parse(
        r#"
openapi: 3.1.0
info: { title: Secured orders, version: 1.0.0 }
paths:
  /orders:
    post:
      operationId: createOrder
      security:
        - merchantOAuth: [orders:write]
        - merchantKey: []
      responses:
        "201": { description: Created }
components:
  securitySchemes:
    merchantOAuth:
      type: oauth2
      flows: {}
    merchantKey:
      type: apiKey
      in: header
      name: X-Merchant-Key
"#,
    )
    .unwrap_or_else(|error| panic!("{error}"))
}
