use serde_json::json;
use simile_compiler::{CompileRequest, Compiler, Source};
use simile_heuristics::Policy;
use simile_spec::{EffectActionKind, EffectOrigin, EffectValue};

#[test]
fn infers_one_conservative_database_write_for_a_created_entity() {
    let service = compile(
        "/v1/products/create",
        "createProduct",
        r#"
                type: object
                required: [product]
                properties:
                  product:
                    $ref: '#/components/schemas/Product'
"#,
        "backend \"db\" \"primary\" {}",
    );

    let effect = service
        .effects
        .values()
        .next()
        .unwrap_or_else(|| panic!("expected an inferred effect"));
    assert_eq!(effect.origin, EffectOrigin::ConventionInferred);
    assert_eq!(effect.operations, ["createProduct"]);
    assert_eq!(effect.actions[0].kind, EffectActionKind::DbPut);
    assert_eq!(effect.actions[0].resource.as_str(), "db.primary");
    assert_eq!(
        effect.actions[0].arguments["collection"],
        EffectValue::Literal {
            value: json!("products")
        }
    );
}

#[test]
fn does_not_guess_when_databases_or_entities_are_ambiguous() {
    let service = compile(
        "/things",
        "createThing",
        "                type: object\n                properties:\n                  ok: { type: boolean }",
        "backend \"db\" \"east\" {}\nbackend \"db\" \"west\" {}",
    );
    assert!(service.effects.is_empty());
}

#[test]
fn requires_the_route_resource_to_match_the_response_entity() {
    for (path, operation_id) in [
        ("/widgets/lookup", "lookupWidget"),
        ("/widgets/page", "pageWidgets"),
        ("/widgets/preview", "previewWidget"),
        ("/widgets/metrics", "measureWidget"),
    ] {
        let service = compile(
            path,
            operation_id,
            r#"
                type: object
                required: [widget]
                properties:
                  widget:
                    $ref: '#/components/schemas/Widget'
"#,
            "backend \"db\" \"primary\" {}",
        );
        assert!(service.effects.is_empty(), "{operation_id} became a write");
    }
}

fn compile(
    path: &str,
    operation_id: &str,
    response_schema: &str,
    behavior: &str,
) -> simile_spec::CompiledService {
    let openapi = format!(
        r#"
openapi: 3.1.2
info: {{ title: Products, version: 1.0.0 }}
paths:
  {path}:
    post:
      operationId: {operation_id}
      responses:
        "201":
          description: Created
          content:
            application/json:
              schema:
{response_schema}
components:
  schemas:
    Product:
      type: object
      required: [id]
      properties:
        id: {{ type: string }}
    Widget:
      type: object
      required: [id]
      properties:
        id: {{ type: string }}
"#
    );
    Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("products.yml", &openapi),
            behaviors: vec![Source::new("products.hcl", behavior)],
            heuristic_policy: Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"))
}
