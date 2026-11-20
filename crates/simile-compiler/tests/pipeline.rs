use simile_compiler::{CompileRequest, Compiler, ContractSource, Source, SourceCompileRequest};
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
fn smithy_and_typescript_use_the_canonical_compiler_pipeline() {
    let smithy = Compiler::new()
        .compile_source(SourceCompileRequest {
            contract: ContractSource::Smithy(Source::new(
                "orders.smithy",
                r#"$version: "2.0"
namespace example.orders

structure CreateOrderInput {}
structure CreateOrderOutput {}

@http(method: "POST", uri: "/orders", code: 201)
operation createOrder {
  input: CreateOrderInput
  output: CreateOrderOutput
}

service Orders {
  version: "1.0.0"
  operations: [createOrder]
}
"#,
            )),
            behaviors: Vec::new(),
            heuristic_policy: simile_heuristics::Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("Smithy must compile: {error}"));
    let typescript = Compiler::new()
        .compile_source(SourceCompileRequest {
            contract: ContractSource::TypeScript(vec![Source::new(
                "orders.d.ts",
                r#"
export type ServiceName = "Orders";
export type CollectionVersion = "1.0.0";
export interface OrdersOperations {
  readonly createOrder: {
    readonly method: "POST";
    readonly path: "/orders";
    readonly responses: { readonly "201": { readonly content: {
      readonly "application/json": { readonly id: string };
    } } };
  };
}
"#,
            )]),
            behaviors: Vec::new(),
            heuristic_policy: simile_heuristics::Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("TypeScript must compile: {error}"));

    for compiled in [smithy, typescript] {
        assert_eq!(compiled.contract.info.title, "Orders");
        assert!(compiled.contract.operation_by_id("createOrder").is_some());
        assert_eq!(compiled.operations.len(), 1);
    }
}

#[test]
fn hcl_refines_only_the_operation_it_names() {
    let compiled = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("orders.yaml", openapi()),
            behaviors: vec![Source::new(
                "orders.hcl",
                r#"
backend "queue" "orders" {}

async "dispatch_order" {
  uses = ["queue.orders"]
}

behavior "createOrder" {
  response { status = 201 }
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
            .and_then(|value| value.response_status),
        Some(201)
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

#[test]
fn derives_named_outcomes_and_marks_schema_only_fallbacks() {
    let source = r#"
openapi: 3.1.2
info: { title: Payments, version: 1.0.0 }
paths:
  /payments:
    post:
      operationId: createPayment
      responses:
        "200":
          description: Paid
          content:
            application/json:
              schema: { type: object }
              examples:
                succeeded:
                  value: { status: succeeded }
        "402":
          description: Declined
          content:
            application/json:
              schema: { type: object }
"#;
    let compiled = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("payments.yaml", source),
            behaviors: Vec::new(),
            heuristic_policy: simile_heuristics::Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    let outcomes = &compiled
        .operations
        .values()
        .next()
        .unwrap_or_else(|| panic!("compiled operation missing"))
        .outcomes;

    assert_eq!(outcomes[0].name, "succeeded");
    assert_eq!(
        outcomes[0].provenance,
        simile_spec::OutcomeProvenance::OpenApiDerived
    );
    assert_eq!(outcomes[1].name, "http_402");
    assert_eq!(
        outcomes[1].provenance,
        simile_spec::OutcomeProvenance::SchemaSynthesized
    );
}

#[test]
fn applies_reusable_error_sets_by_openapi_tag() {
    let source = r#"
openapi: 3.1.2
info: { title: Orders, version: 1.0.0 }
paths:
  /orders:
    post:
      operationId: createOrder
      tags: [orders]
      responses:
        "201": { description: Created }
        "409": { description: Conflict }
"#;
    let behavior = r#"
error_set "write_conflicts" {
  outcome "conflict" { status = 409 }
}

apply_error_set "write_conflicts" {
  tags = ["orders"]
}
"#;
    let compiled = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("orders.yaml", source),
            behaviors: vec![Source::new("orders.hcl", behavior)],
            heuristic_policy: simile_heuristics::Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    let operation = compiled
        .operations
        .values()
        .next()
        .unwrap_or_else(|| panic!("compiled operation missing"));

    assert!(
        operation
            .outcomes
            .iter()
            .any(|outcome| outcome.name == "conflict")
    );
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
