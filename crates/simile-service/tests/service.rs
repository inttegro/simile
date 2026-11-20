use simile_artifact::{decode, encode};
use simile_compiler::{CompileRequest, Compiler, Source};
use simile_heuristics::Policy;
use simile_runtime::{ExecutionOptions, OperationCall, ReplayIdentity};
use simile_service::Service;
use simile_stack::{
    CommandMetadata, DbCommand, DbResult, ResourceAddress, Stack, StackCommand, StackResult,
};

#[test]
fn commits_declared_effects_and_does_not_repeat_them_on_replay() {
    let service = Service::memory(artifact()).unwrap_or_else(|error| panic!("{error}"));
    let call = OperationCall {
        operation: "POST /orders",
        content_type: Some("application/json"),
        body: br#"{"product":"beat_pack"}"#,
    };
    let options = ExecutionOptions {
        replay: Some(ReplayIdentity::IdempotencyKey("checkout-1")),
        ..ExecutionOptions::default()
    };

    let first = service
        .execute_with_options(call, options)
        .unwrap_or_else(|error| panic!("{error}"));
    let key = first.response().value().and_then(|body| body.get("id"));
    let key = key
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("generated response should contain an id"));
    service
        .execute_with_options(call, options)
        .unwrap_or_else(|error| panic!("{error}"));

    let mut stack =
        Stack::from_snapshot(service.snapshot().unwrap_or_else(|error| panic!("{error}")));
    let receipt = stack
        .execute(
            "db.orders"
                .parse::<ResourceAddress>()
                .unwrap_or_else(|error| panic!("{error}")),
            CommandMetadata::default(),
            StackCommand::Db(DbCommand::Get {
                collection: "orders".to_owned(),
                key: key.to_owned(),
            }),
        )
        .unwrap_or_else(|error| panic!("{error}"));

    let StackResult::Db(result) = receipt.result else {
        panic!("expected a database result");
    };
    let DbResult::Document(Some(document)) = *result else {
        panic!("expected the persisted order");
    };
    assert_eq!(document.version, 1);
    assert_eq!(
        Some(&document.value),
        first.response().value(),
        "stored value should be the committed response"
    );
}

fn artifact() -> simile_artifact::Artifact {
    let compiled = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new(
                "orders.yml",
                r#"
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
              required: [product]
              properties:
                product: { type: string }
      responses:
        "201":
          description: Created
          content:
            application/json:
              schema:
                type: object
                required: [id, product]
                properties:
                  id: { type: string, minLength: 12 }
                  product: { type: string }
"#,
            ),
            behaviors: vec![Source::new(
                "orders.hcl",
                r#"
backend "db" "orders" {}

effect "persist_order" {
  operations = ["createOrder"]

  action "db.orders" "put" {
    collection = "orders"
    key        = response.id
    value      = response
  }
}

behavior "createOrder" {
  response { status = 201 }
}
"#,
            )],
            heuristic_policy: Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    let encoded = encode(&compiled).unwrap_or_else(|error| panic!("{error}"));
    decode(&encoded).unwrap_or_else(|error| panic!("{error}"))
}
