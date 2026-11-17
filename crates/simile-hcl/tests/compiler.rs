use simile_hcl::Source;
use simile_spec::{ResourceKind, ResourceRef};

#[test]
fn parses_and_compiles_behavior_modules() {
    let api = contract();
    let module = simile_hcl::parse(Source::new(
        "orders.hcl",
        r#"
resource "db" "orders" {}
resource "event_bus" "commerce" {}

async "publish_order" {
  uses = ["db.orders", "event_bus.commerce"]
}

behavior "createOrder" {
  response {
    status = 201
  }

  trigger "publish_order" {}
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
fn rejects_provider_specific_resources() {
    let error = parse_error(r#"resource "postgres" "orders" {}"#);

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
fn rejects_unknown_async_and_resource_references() {
    let behavior = module(
        r#"
behavior "createOrder" {
  trigger "missing" {}
}
"#,
    );
    let error = compile_error([behavior]);
    assert!(error.to_string().contains("unknown async operation"));

    let operation = module(
        r#"
async "publish_order" {
  uses = ["queue.missing"]
}
"#,
    );
    let error = compile_error([operation]);
    assert!(error.to_string().contains("undeclared resource"));
}

#[test]
fn resolves_cross_module_references_without_file_ordering() {
    let behavior = module(
        r#"
behavior "createOrder" {
  trigger "publish_order" {}
}
"#,
    );
    let resources = module(
        r#"
async "publish_order" {
  uses = ["queue.orders"]
}

resource "queue" "orders" {}
"#,
    );

    let compiled = simile_hcl::compile([behavior, resources], &contract())
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
