use simile_artifact::{decode, encode};
use simile_compiler::{CompileRequest, Compiler, Source};
use simile_heuristics::Policy;
use simile_runtime::{ExecutionOptions, OperationCall, OperationValueCall, ReplayIdentity};
use simile_runtime::{IncomingParameters, validate_parameters};
use simile_service::{DependencyReadiness, Service};
use simile_stack::{
    CommandMetadata, DbCommand, DbResult, ResourceAddress, Stack, StackCommand, StackResult,
};

#[test]
fn durable_outbox_survives_restart_and_acknowledgement() {
    let path = std::env::temp_dir().join(format!(
        "simile-service-outbox-{}-{}.redb",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let artifact = deferred_work_artifact();
    {
        let service = Service::durable(artifact, &path).unwrap_or_else(|error| panic!("{error}"));
        service
            .execute(OperationCall {
                operation: "POST /orders",
                content_type: None,
                body: &[],
            })
            .unwrap_or_else(|error| panic!("{error}"));
    }

    let artifact = deferred_work_artifact();
    {
        let service = Service::durable(artifact, &path).unwrap_or_else(|error| panic!("{error}"));
        let leases = service
            .lease_work(10, None)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].work().name(), "publish_order");
        service
            .execute_work(&leases[0])
            .unwrap_or_else(|error| panic!("{error}"));
        service
            .acknowledge_work(&leases[0])
            .unwrap_or_else(|error| panic!("{error}"));
    }

    let service =
        Service::durable(deferred_work_artifact(), &path).unwrap_or_else(|error| panic!("{error}"));
    assert!(
        service
            .lease_work(10, Some(0))
            .unwrap_or_else(|error| panic!("{error}"))
            .is_empty()
    );
    drop(service);
    let _ = std::fs::remove_file(path);
}

#[test]
fn hard_webhook_dependencies_wait_without_spending_the_retry_budget() {
    let service =
        Service::memory(webhook_dependency_artifact()).unwrap_or_else(|error| panic!("{error}"));
    service
        .execute(OperationCall {
            operation: "POST /orders",
            content_type: None,
            body: &[],
        })
        .unwrap_or_else(|error| panic!("{error}"));

    let leases = service
        .lease_work(10, None)
        .unwrap_or_else(|error| panic!("{error}"));
    let predecessor = leases
        .iter()
        .find(|lease| lease.work().name() == "payment.succeeded")
        .cloned()
        .unwrap_or_else(|| panic!("missing predecessor"));
    let dependent = leases
        .iter()
        .find(|lease| lease.work().name() == "order.paid")
        .cloned()
        .unwrap_or_else(|| panic!("missing dependent"));
    assert_eq!(
        service
            .dependency_readiness(&dependent)
            .unwrap_or_else(|error| panic!("{error}")),
        DependencyReadiness::Waiting
    );
    service
        .defer_work(&dependent, 0)
        .unwrap_or_else(|error| panic!("{error}"));
    service
        .acknowledge_work(&predecessor)
        .unwrap_or_else(|error| panic!("{error}"));

    let dependent = service
        .lease_work(10, None)
        .unwrap_or_else(|error| panic!("{error}"))
        .into_iter()
        .find(|lease| lease.work().name() == "order.paid")
        .unwrap_or_else(|| panic!("missing deferred dependent"));
    assert_eq!(dependent.attempt(), 1);
    assert_eq!(
        service
            .dependency_readiness(&dependent)
            .unwrap_or_else(|error| panic!("{error}")),
        DependencyReadiness::Ready
    );
    service
        .acknowledge_work(&dependent)
        .unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn failed_work_exhausts_its_budget_and_redelivery_starts_fresh() {
    let service =
        Service::memory(deferred_work_artifact()).unwrap_or_else(|error| panic!("{error}"));
    service
        .execute(OperationCall {
            operation: "POST /orders",
            content_type: None,
            body: &[],
        })
        .unwrap_or_else(|error| panic!("{error}"));

    for expected_attempt in 1..=5 {
        let lease = service
            .lease_work(1, None)
            .unwrap_or_else(|error| panic!("{error}"))
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("missing retry attempt {expected_attempt}"));
        assert_eq!(lease.attempt(), expected_attempt);
        let retrying = service
            .retry_work(&lease, 0)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(retrying, expected_attempt < 5);
    }

    let dead_letter = service
        .dead_letters(1)
        .unwrap_or_else(|error| panic!("{error}"))
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("missing dead letter"));
    assert_eq!(dead_letter.attempts(), 5);
    service
        .redeliver(&dead_letter)
        .unwrap_or_else(|error| panic!("{error}"));
    let lease = service
        .lease_work(1, None)
        .unwrap_or_else(|error| panic!("{error}"))
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("missing redelivered work"));
    assert_eq!(lease.attempt(), 1);
}

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

#[test]
fn inferred_entities_survive_create_read_list_and_delete() {
    let service = service_with_contract(
        r#"
openapi: 3.1.0
info: { title: Products, version: 1.0.0 }
paths:
  /products:
    post:
      operationId: createProduct
      responses:
        "201":
          description: created
          content:
            application/json:
              schema: { $ref: '#/components/schemas/Product' }
    get:
      operationId: listProducts
      responses:
        "200":
          description: listed
          content:
            application/json:
              schema:
                type: array
                items: { $ref: '#/components/schemas/Product' }
  /products/{id}:
    parameters:
      - name: id
        in: path
        required: true
        schema: { type: string }
    get:
      operationId: getProduct
      responses:
        "200":
          description: found
          content:
            application/json:
              schema: { $ref: '#/components/schemas/Product' }
        "404": { description: missing }
    delete:
      operationId: deleteProduct
      responses:
        "204": { description: deleted }
components:
  schemas:
    Product:
      type: object
      required: [id, name]
      properties:
        id: { type: string, pattern: '^prod_' }
        name: { type: string }
"#,
        "backend \"db\" \"primary\" {}",
    );

    let created = service
        .execute(OperationCall {
            operation: "POST /products",
            content_type: None,
            body: &[],
        })
        .unwrap_or_else(|error| panic!("{error}"));
    let entity = created
        .response()
        .value()
        .cloned()
        .unwrap_or_else(|| panic!("expected product"));
    let id = entity
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("expected product id"));
    let parameters = product_parameters(&service, "GET /products/{id}", id);

    let found = service
        .execute_with_options(
            OperationCall {
                operation: "GET /products/{id}",
                content_type: None,
                body: &[],
            },
            ExecutionOptions {
                parameters: Some(&parameters),
                ..ExecutionOptions::default()
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(found.response().value(), Some(&entity));

    let listed = service
        .execute(OperationCall {
            operation: "GET /products",
            content_type: None,
            body: &[],
        })
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        listed.response().value(),
        Some(&serde_json::json!([entity]))
    );

    let delete_parameters = product_parameters(&service, "DELETE /products/{id}", id);
    service
        .execute_with_options(
            OperationCall {
                operation: "DELETE /products/{id}",
                content_type: None,
                body: &[],
            },
            ExecutionOptions {
                parameters: Some(&delete_parameters),
                ..ExecutionOptions::default()
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));

    let missing = service
        .execute_with_options(
            OperationCall {
                operation: "GET /products/{id}",
                content_type: None,
                body: &[],
            },
            ExecutionOptions {
                parameters: Some(&parameters),
                ..ExecutionOptions::default()
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(missing.response().status(), 404);
}

#[test]
fn inferred_entities_survive_post_lookup_and_page_routes() {
    let service = service_with_contract(
        r#"
openapi: 3.1.0
info: { title: Products, version: 1.0.0 }
paths:
  /products/create:
    post:
      operationId: createProduct
      responses:
        "201":
          description: created
          content:
            application/json:
              schema:
                type: object
                required: [product]
                properties:
                  product: { $ref: '#/components/schemas/Product' }
  /products/lookup:
    post:
      operationId: lookupProduct
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              required: [product_id]
              properties:
                product_id: { type: string }
      responses:
        "200":
          description: found
          content:
            application/json:
              schema:
                type: object
                required: [product]
                properties:
                  product: { $ref: '#/components/schemas/Product' }
        "404": { description: missing }
  /products/page:
    post:
      operationId: pageProducts
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              properties:
                page_number: { type: integer, minimum: 1, default: 1 }
      responses:
        "200":
          description: listed
          content:
            application/json:
              schema:
                type: object
                required: [page]
                properties:
                  page:
                    type: object
                    required: [number, size, products]
                    properties:
                      number: { type: integer, minimum: 1 }
                      size: { type: integer, minimum: 0 }
                      products:
                        type: array
                        items: { $ref: '#/components/schemas/Product' }
components:
  schemas:
    Product:
      type: object
      required: [id, name]
      properties:
        id: { type: string, pattern: '^prod_' }
        name: { type: string }
"#,
        r#"
backend "db" "primary" {}

effect "persist_product" {
  operations = ["createProduct"]

  action "db.primary" "put" {
    collection = "products"
    key        = response.product.id
    value      = response.product
  }
}
"#,
    );

    let created = service
        .execute(OperationCall {
            operation: "POST /products/create",
            content_type: None,
            body: &[],
        })
        .unwrap_or_else(|error| panic!("{error}"));
    let product = created
        .response()
        .value()
        .and_then(|body| body.get("product"))
        .cloned()
        .unwrap_or_else(|| panic!("expected product"));
    let id = product
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("expected product id"));

    let lookup_body = serde_json::json!({"product_id": id});
    let found = service
        .execute_value(OperationValueCall {
            operation: "POST /products/lookup",
            content_type: Some("application/json"),
            body: Some(&lookup_body),
        })
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        found.response().value(),
        Some(&serde_json::json!({"product": product}))
    );

    let page_body = serde_json::json!({"page_number": 1});
    let listed = service
        .execute_value(OperationValueCall {
            operation: "POST /products/page",
            content_type: Some("application/json"),
            body: Some(&page_body),
        })
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        listed.response().value(),
        Some(&serde_json::json!({
            "page": {"number": 1, "size": 1, "products": [product]}
        }))
    );
}

#[test]
fn stateful_mutation_projects_onto_the_persisted_snapshot() {
    let service = service_with_contract(
        r#"
openapi: 3.1.0
info: { title: Products, version: 1.0.0 }
paths:
  /products/create:
    post:
      operationId: createProduct
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              required: [type, name]
              properties:
                type: { type: string, enum: [digital, physical] }
                name: { type: string }
      responses:
        "200":
          description: created
          content:
            application/json:
              schema: { $ref: '#/components/schemas/ProductResponse' }
              x-simile-from-request:
                bindings:
                  - { from: /type, to: /product/type }
                  - { from: /name, to: /product/name }
                  - { to: /product/active, value: { literal: true } }
  /products/update:
    post:
      operationId: updateProduct
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              required: [product_id]
              properties:
                product_id: { type: string, pattern: '^prod_' }
                name: { type: string }
      responses:
        "200":
          description: updated
          content:
            application/json:
              schema: { $ref: '#/components/schemas/UpdatedProductResponse' }
              x-simile-from-request:
                bindings:
                  - { from: /product_id, to: /product/id }
                  - { from: /name, to: /product/name }
  /products/lookup:
    post:
      operationId: lookupProduct
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              required: [product_id]
              properties:
                product_id: { type: string, pattern: '^prod_' }
      responses:
        "200":
          description: found
          content:
            application/json:
              schema: { $ref: '#/components/schemas/ProductResponse' }
components:
  schemas:
    ProductResponse:
      type: object
      required: [product]
      properties:
        product: { $ref: '#/components/schemas/Product' }
    UpdatedProductResponse:
      type: object
      required: [product]
      properties:
        product: { $ref: '#/components/schemas/UpdatedProduct' }
    UpdatedProduct:
      type: object
      additionalProperties: false
      required: [id, type, name]
      properties:
        id: { type: string, pattern: '^prod_' }
        type: { type: string, enum: [digital, physical] }
        name: { type: string }
    Product:
      type: object
      required: [id, type, name, active]
      properties:
        id: { type: string, pattern: '^prod_' }
        type: { type: string, enum: [digital, physical] }
        name: { type: string }
        active: { type: boolean }
"#,
        r#"
backend "db" "primary" {}

effect "persist_product" {
  operations = ["createProduct", "updateProduct"]

  action "db.primary" "merge" {
    collection = "products"
    key        = response.product.id
    value      = response.product
  }
}
"#,
    );

    let create = serde_json::json!({"type": "digital", "name": "First edition"});
    let created = service
        .execute_value(OperationValueCall {
            operation: "POST /products/create",
            content_type: Some("application/json"),
            body: Some(&create),
        })
        .unwrap_or_else(|error| panic!("{error}"));
    let product = created
        .response()
        .value()
        .and_then(|body| body.get("product"))
        .cloned()
        .unwrap_or_else(|| panic!("expected product"));
    let id = product
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("expected product id"));
    assert_eq!(product.get("type"), Some(&serde_json::json!("digital")));
    assert_eq!(product.get("active"), Some(&serde_json::json!(true)));

    let update = serde_json::json!({"product_id": id, "name": "Second edition"});
    let updated = service
        .execute_value(OperationValueCall {
            operation: "POST /products/update",
            content_type: Some("application/json"),
            body: Some(&update),
        })
        .unwrap_or_else(|error| panic!("{error}"));
    let product = updated
        .response()
        .value()
        .and_then(|body| body.get("product"))
        .cloned()
        .unwrap_or_else(|| panic!("expected updated product"));
    assert_eq!(product.get("id"), Some(&serde_json::json!(id)));
    assert_eq!(
        product.get("name"),
        Some(&serde_json::json!("Second edition"))
    );
    assert_eq!(product.get("type"), Some(&serde_json::json!("digital")));
    // Why: the mutation response must remain faithful to its deliberately narrow public schema;
    // preservation is proven through the subsequent full-resource lookup instead.
    assert_eq!(product.get("active"), None);

    let lookup = serde_json::json!({"product_id": id});
    let found = service
        .execute_value(OperationValueCall {
            operation: "POST /products/lookup",
            content_type: Some("application/json"),
            body: Some(&lookup),
        })
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        found.response().value(),
        Some(&serde_json::json!({
            "product": {
                "id": id,
                "name": "Second edition",
                "type": "digital",
                "active": true
            }
        }))
    );
}

#[test]
fn get_actions_fall_through_when_state_cannot_represent_the_response() {
    let service = service_with_contract(
        r#"
openapi: 3.1.0
info: { title: Widgets, version: 1.0.0 }
paths:
  /widgets/export:
    get:
      operationId: exportWidgets
      responses:
        "200":
          description: export ready
          content:
            application/json:
              schema:
                type: object
                required: [download_url]
                properties:
                  download_url: { type: string, format: uri }
"#,
        "backend \"db\" \"primary\" {}",
    );

    let exported = service
        .execute(OperationCall {
            operation: "GET /widgets/export",
            content_type: None,
            body: &[],
        })
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(exported.response().status(), 200);
    assert!(
        exported
            .response()
            .value()
            .and_then(|body| body.get("download_url"))
            .is_some()
    );
}

fn product_parameters(
    service: &simile_service::Service,
    operation: &str,
    id: &str,
) -> simile_runtime::ValidatedParameters {
    validate_parameters(
        service.artifact(),
        operation,
        &IncomingParameters {
            path: [("id".to_owned(), vec![id.to_owned()])]
                .into_iter()
                .collect(),
            ..IncomingParameters::default()
        },
    )
    .unwrap_or_else(|error| panic!("{error}"))
}

fn service_with_contract(openapi: &str, behavior: &str) -> simile_service::Service {
    let compiled = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("service.yml", openapi),
            behaviors: vec![Source::new("service.hcl", behavior)],
            heuristic_policy: Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    let encoded = simile_artifact::encode(&compiled).unwrap_or_else(|error| panic!("{error}"));
    let artifact = simile_artifact::decode(&encoded).unwrap_or_else(|error| panic!("{error}"));
    simile_service::Service::memory(artifact).unwrap_or_else(|error| panic!("{error}"))
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

fn deferred_work_artifact() -> simile_artifact::Artifact {
    let compiled = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new(
                "orders.yml",
                r#"
openapi: 3.1.0
info: { title: Durable orders, version: 1.0.0 }
paths:
  /orders:
    post:
      operationId: createOrder
      responses:
        "201":
          description: Created
          content:
            application/json:
              schema:
                type: object
                required: [id]
                properties:
                  id: { type: string, pattern: '^ord_' }
"#,
            ),
            behaviors: vec![Source::new(
                "orders.hcl",
                r#"
backend "event_bus" "events" {}

async "publish_order" {
  action "event_bus.events" "publish" {
    topic = "orders.created"
    body  = response
  }
}

effect "publish_created_order" {
  operations = ["createOrder"]
  async      = ["publish_order"]
}
"#,
            )],
            heuristic_policy: Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    let encoded = encode(&compiled).unwrap_or_else(|error| panic!("{error}"));
    decode(&encoded).unwrap_or_else(|error| panic!("{error}"))
}

fn webhook_dependency_artifact() -> simile_artifact::Artifact {
    let compiled = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new(
                "orders.yml",
                r#"
openapi: 3.1.0
info: { title: Webhook orders, version: 1.0.0 }
paths:
  /orders:
    post:
      operationId: createOrder
      responses:
        "201":
          description: Created
          content:
            application/json:
              schema:
                type: object
                required: [id]
                properties:
                  id: { type: string, pattern: '^ord_' }
"#,
            ),
            behaviors: vec![Source::new(
                "orders.hcl",
                r#"
webhook "payment.succeeded" {}
webhook "order.paid" {}

effect "announce_order" {
  operations = ["createOrder"]
  emit = {
    "payment.succeeded" = response
    "order.paid"        = response
  }
  after = {
    "order.paid" = ["payment.succeeded"]
  }
  to = {
    "payment.succeeded" = "http://127.0.0.1:9/hooks"
    "order.paid"        = "http://127.0.0.1:9/hooks"
  }
}
"#,
            )],
            heuristic_policy: Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    let encoded = encode(&compiled).unwrap_or_else(|error| panic!("{error}"));
    decode(&encoded).unwrap_or_else(|error| panic!("{error}"))
}
