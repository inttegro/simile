use simile_compiler::{CompileRequest, Compiler, Source};
use simile_spec::BehaviorOrigin;

#[test]
fn openapi_is_a_complete_default_behavior_source() {
    let compiled = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("orders.yaml", openapi()),
            behaviors: Vec::new(),
            heuristic_policy: simile_heuristics::Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(compiled.operations.len(), 1);
    let operation = compiled
        .operations
        .values()
        .next()
        .unwrap_or_else(|| panic!("compiled operation missing"));
    assert_eq!(operation.origin, BehaviorOrigin::OpenApi);
    assert!(operation.behavior.is_none());
}

#[test]
fn hcl_refines_only_the_operation_it_names() {
    let compiled = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("orders.yaml", openapi()),
            behaviors: vec![Source::new(
                "orders.hcl",
                r#"
resource "queue" "orders" {}

async "dispatch_order" {
  uses = ["queue.orders"]
}

behavior "createOrder" {
  response { status = 201 }
  trigger "dispatch_order" {}
}
"#,
            )],
            heuristic_policy: simile_heuristics::Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));

    let operation = compiled
        .operations
        .values()
        .next()
        .unwrap_or_else(|| panic!("compiled operation missing"));
    assert_eq!(operation.origin, BehaviorOrigin::Hcl);
    assert_eq!(
        operation
            .behavior
            .as_ref()
            .map(|value| value.triggers.len()),
        Some(1)
    );
    assert_eq!(compiled.resources.len(), 1);
    assert_eq!(compiled.async_operations.len(), 1);
}

#[test]
fn an_operation_without_an_id_still_gets_openapi_behavior() {
    let source = r#"
openapi: 3.2.1
info:
  title: Health
  version: 1.0.0
paths:
  /health:
    get:
      responses:
        "200": { description: Healthy }
"#;
    let compiled = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("health.yaml", source),
            behaviors: Vec::new(),
            heuristic_policy: simile_heuristics::Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));

    let operation = compiled
        .operations
        .values()
        .next()
        .unwrap_or_else(|| panic!("compiled operation missing"));
    assert_eq!(operation.origin, BehaviorOrigin::OpenApi);
    assert!(operation.contract.operation_id.is_none());
}

fn openapi() -> &'static str {
    r#"
openapi: 3.2.1
info:
  title: Orders
  version: 1.0.0
paths:
  /orders:
    post:
      operationId: createOrder
      responses:
        "201": { description: Created }
"#
}
