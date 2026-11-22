use std::collections::BTreeMap;

use serde_json::json;
use simile_compiler::{CompileRequest, Compiler, Source};
use simile_heuristics::Policy;
use simile_runtime::{IncomingParameters, validate_parameters};
use simile_spec::ParameterLocation;

fn artifact() -> simile_artifact::Artifact {
    let source = r#"
openapi: 3.1.0
info: { title: Parameter encodings, version: 1.0.0 }
paths:
  /paint/{color}:
    get:
      operationId: paint
      parameters:
        - name: color
          in: path
          required: true
          style: matrix
          explode: true
          schema:
            type: object
            required: [r, g]
            properties:
              r: { type: integer }
              g: { type: integer }
        - name: filter
          in: query
          style: form
          explode: true
          schema:
            type: object
            required: [role, active]
            properties:
              role: { type: string }
              active: { type: boolean }
        - name: X-Meta
          in: header
          style: simple
          explode: true
          schema:
            type: object
            required: [region, level]
            properties:
              region: { type: string }
              level: { type: integer }
      responses:
        '204': { description: painted }
  /points/{point}:
    get:
      operationId: point
      parameters:
        - name: point
          in: path
          required: true
          style: label
          explode: true
          schema:
            type: array
            items: { type: integer }
      responses:
        '204': { description: found }
"#;
    let compiled = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("parameters.yml", source),
            modules: Vec::new(),
            heuristic_policy: Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    let encoded = simile_artifact::encode(&compiled).unwrap_or_else(|error| panic!("{error}"));
    simile_artifact::decode(&encoded).unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn decodes_matrix_form_and_simple_objects() {
    let parameters = validate_parameters(
        &artifact(),
        "GET /paint/{color}",
        &IncomingParameters {
            path: BTreeMap::from([("color".to_owned(), vec![";r=100;g=200".to_owned()])]),
            query: BTreeMap::from([
                ("role".to_owned(), vec!["admin".to_owned()]),
                ("active".to_owned(), vec!["true".to_owned()]),
                ("unrelated".to_owned(), vec!["ignored".to_owned()]),
            ]),
            headers: BTreeMap::from([("x-meta".to_owned(), vec!["region=eu,level=3".to_owned()])]),
            cookies: BTreeMap::new(),
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(
        parameters.get(ParameterLocation::Path, "color"),
        Some(&json!({ "r": 100, "g": 200 }))
    );
    assert_eq!(
        parameters.get(ParameterLocation::Query, "filter"),
        Some(&json!({ "role": "admin", "active": true }))
    );
    assert_eq!(
        parameters.get(ParameterLocation::Header, "X-Meta"),
        Some(&json!({ "region": "eu", "level": 3 }))
    );
}

#[test]
fn decodes_exploded_label_arrays() {
    let parameters = validate_parameters(
        &artifact(),
        "GET /points/{point}",
        &IncomingParameters {
            path: BTreeMap::from([("point".to_owned(), vec![".3.4.5".to_owned()])]),
            ..IncomingParameters::default()
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(
        parameters.get(ParameterLocation::Path, "point"),
        Some(&json!([3, 4, 5]))
    );
}
