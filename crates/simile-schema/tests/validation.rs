use std::collections::BTreeMap;

use serde_json::json;
use simile_schema::{
    BodySchemaCompiler, BoundRequestBody, BoundResponseBody, RequestBodyPlan, SchemaCatalog,
    ValidationRule,
};
use simile_spec::{MediaTypeContract, OpenApiFeature, RequestBodyContract, ResponseBodyContract};

#[test]
fn validates_recursive_component_schemas() {
    let components = BTreeMap::from([(
        "Node".to_owned(),
        json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": {"type": "string", "minLength": 1},
                "children": {
                    "type": "array",
                    "items": {"$ref": "#/components/schemas/Node"}
                }
            }
        }),
    )]);
    let (plan, schemas) = plan(
        json!({"$ref": "#/components/schemas/Node"}),
        components,
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    assert!(
        media
            .validate(&json!({"name": "root", "children": [{"name": "leaf"}]}))
            .is_empty()
    );
    let issues = media.validate(&json!({"name": "root", "children": [{}]}));
    assert_eq!(issues[0].path, "/children/0/name");
    assert_eq!(issues[0].rule, ValidationRule::Required);
}

#[test]
fn enforces_combinators_and_openapi_30_nullable() {
    let (plan, schemas) = plan(
        json!({
            "type": "string",
            "nullable": true,
            "oneOf": [
                {"enum": ["draft"]},
                {"enum": ["ready"]}
            ]
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_0,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    assert!(
        media
            .validate(&json!(null))
            .iter()
            .any(|issue| { issue.rule == ValidationRule::OneOf })
    );
    assert!(media.validate(&json!("ready")).is_empty());
    assert_eq!(media.validate(&json!(42))[0].rule, ValidationRule::Type);
}

#[test]
fn matches_content_type_parameters_and_wildcards() {
    let body = RequestBodyContract {
        required: false,
        content: BTreeMap::from([
            (
                "application/*".to_owned(),
                MediaTypeContract {
                    schema: None,
                    example: None,
                },
            ),
            (
                "*/*".to_owned(),
                MediaTypeContract {
                    schema: None,
                    example: None,
                },
            ),
        ]),
    };
    let components = BTreeMap::new();
    let mut compiler = BodySchemaCompiler::new(&components, OpenApiFeature::V3_1);
    let plan = compiler
        .compile_request(&body)
        .unwrap_or_else(|error| panic!("{error}"));
    let schemas = compiler.finish().unwrap_or_else(|error| panic!("{error}"));
    let body = BoundRequestBody::new(&plan, &schemas);

    assert_eq!(
        body.select("Application/JSON; charset=utf-8")
            .map(|media| media.declared()),
        Some("application/*")
    );
    assert_eq!(
        body.select("text/plain").map(|media| media.declared()),
        Some("*/*")
    );
}

#[test]
fn shares_component_nodes_across_request_bodies() {
    let components = BTreeMap::from([(
        "Customer".to_owned(),
        json!({
            "type": "object",
            "properties": {"email": {"type": "string"}}
        }),
    )]);
    let body = RequestBodyContract {
        required: true,
        content: BTreeMap::from([(
            "application/json".to_owned(),
            MediaTypeContract {
                schema: Some(json!({"$ref": "#/components/schemas/Customer"})),
                example: None,
            },
        )]),
    };
    let mut compiler = BodySchemaCompiler::new(&components, OpenApiFeature::V3_1);
    let first = compiler
        .compile_request(&body)
        .unwrap_or_else(|error| panic!("{error}"));
    let second = compiler
        .compile_request(&body)
        .unwrap_or_else(|error| panic!("{error}"));
    let schemas = compiler.finish().unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(schemas.len(), 2);
    assert!(BoundRequestBody::new(&first, &schemas).is_well_formed());
    assert!(BoundRequestBody::new(&second, &schemas).is_well_formed());
}

#[test]
fn does_not_require_response_only_properties_in_requests() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["id", "name"],
            "properties": {
                "id": {"type": "string", "readOnly": true},
                "name": {"type": "string"}
            }
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_0,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    assert!(media.validate(&json!({"name": "Ada"})).is_empty());
    assert_eq!(media.validate(&json!({}))[0].path, "/name");
}

#[test]
fn projects_compatible_request_values_without_reflecting_write_only_fields() {
    let request = RequestBodyContract {
        required: true,
        content: BTreeMap::from([(
            "application/json".to_owned(),
            MediaTypeContract {
                schema: Some(json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["name", "secret", "profile"],
                    "properties": {
                        "name": {"type": "string"},
                        "secret": {"$ref": "#/components/schemas/Secret"},
                        "profile": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["note"],
                            "properties": {"note": {"type": "string"}}
                        }
                    }
                })),
                example: None,
            },
        )]),
    };
    let response = ResponseBodyContract {
        content: BTreeMap::from([(
            "application/json".to_owned(),
            MediaTypeContract {
                schema: Some(json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "name", "secret", "profile"],
                    "properties": {
                        "id": {"type": "string", "example": "ord_simile"},
                        "name": {"type": "string"},
                        "secret": {"$ref": "#/components/schemas/Secret"},
                        "profile": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["note", "level"],
                            "properties": {
                                "note": {"type": "string"},
                                "level": {"type": "string", "enum": ["new"]}
                            }
                        }
                    }
                })),
                example: None,
            },
        )]),
    };
    let components = BTreeMap::from([
        (
            "Secret".to_owned(),
            json!({"allOf": [{"$ref": "#/components/schemas/SecretValue"}]}),
        ),
        (
            "SecretValue".to_owned(),
            json!({"type": "string", "writeOnly": true}),
        ),
    ]);
    let mut compiler = BodySchemaCompiler::new(&components, OpenApiFeature::V3_1);
    let request_plan = compiler
        .compile_request(&request)
        .unwrap_or_else(|error| panic!("{error}"));
    let response_plan = compiler
        .compile_response(&response)
        .unwrap_or_else(|error| panic!("{error}"));
    let schemas = compiler.finish().unwrap_or_else(|error| panic!("{error}"));
    let request_media = BoundRequestBody::new(&request_plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("request media type should be present"));
    let response_media = BoundResponseBody::new(&response_plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("response media type should be present"));
    let input = json!({
        "name": "Night Drive",
        "secret": "do-not-reflect",
        "profile": {"note": "customer supplied"}
    });

    assert!(request_media.validate(&input).is_empty());
    let value = response_media
        .synthesize_from(&input)
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(value["id"], "ord_simile");
    assert_eq!(value["name"], "Night Drive");
    assert_eq!(value["profile"]["note"], "customer supplied");
    assert_eq!(value["profile"]["level"], "new");
    assert!(value.get("secret").is_none());
    assert!(response_media.validate(&value).is_empty());
}

#[test]
fn rejects_empty_type_arrays() {
    let body = RequestBodyContract {
        required: true,
        content: BTreeMap::from([(
            "application/json".to_owned(),
            MediaTypeContract {
                schema: Some(json!({"type": []})),
                example: None,
            },
        )]),
    };
    let components = BTreeMap::new();
    let mut compiler = BodySchemaCompiler::new(&components, OpenApiFeature::V3_1);
    let error = match compiler.compile_request(&body) {
        Err(error) => error,
        Ok(plan) => panic!("empty type array should fail, compiled {plan:?}"),
    };

    assert!(error.to_string().contains("array of strings"));
}

#[test]
fn synthesizes_nested_required_values_and_combinators() {
    let (plan, schemas) = plan(
        json!({
            "allOf": [
                {
                    "type": "object",
                    "required": ["customer"],
                    "properties": {
                        "customer": {
                            "type": "object",
                            "required": ["email"],
                            "properties": {
                                "email": {"type": "string", "minLength": 3}
                            }
                        }
                    }
                },
                {
                    "type": "object",
                    "required": ["quantity"],
                    "properties": {
                        "quantity": {"type": "integer", "minimum": 2}
                    }
                }
            ]
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    let value = media.synthesize().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(value["customer"]["email"], "example");
    assert_eq!(value["quantity"], 2);
    assert!(media.validate(&value).is_empty());
}

#[test]
fn representative_synthesis_includes_finite_optional_properties() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "description": {"type": "string"},
                "email": {"type": "string", "format": "email"},
                "name": {"type": "string"}
            }
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    assert_eq!(media.synthesize().ok(), Some(json!({})));
    let value = media
        .synthesize_representative()
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        value,
        json!({
            "description": "example",
            "email": "user@example.com",
            "name": "example"
        })
    );
    assert!(media.validate(&value).is_empty());
}

#[test]
fn synthesizes_explicit_formats_without_reading_descriptions() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": [
                "created",
                "available_at",
                "currency",
                "decimal",
                "email",
                "identifier",
                "description_only"
            ],
            "properties": {
                "created": {
                    "type": "integer",
                    "format": "unix-time",
                    "description": "Seconds since the Unix epoch"
                },
                "available_at": {"type": "string", "format": "date-time"},
                "currency": {"type": "string", "format": "currency"},
                "decimal": {"type": "string", "format": "decimal"},
                "email": {"type": "string", "format": "email"},
                "identifier": {"type": "string", "format": "uuid"},
                "description_only": {
                    "type": "string",
                    "description": "An email address"
                }
            }
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    let value = media.synthesize().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(value["created"], 0);
    assert_eq!(value["available_at"], "1970-01-01T00:00:00Z");
    assert_eq!(value["currency"], "usd");
    assert_eq!(value["decimal"], "0");
    assert_eq!(value["email"], "user@example.com");
    assert_eq!(value["identifier"], "00000000-0000-0000-0000-000000000000");
    assert_eq!(value["description_only"], "example");
    assert!(media.validate(&value).is_empty());
}

#[test]
fn uses_schema_hints_only_when_they_are_valid() {
    let (hinted_plan, hinted_schemas) = plan(
        json!({"type": "string", "minLength": 3, "example": "ready"}),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let hinted = BoundRequestBody::new(&hinted_plan, &hinted_schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));
    assert_eq!(hinted.synthesize().ok(), Some(json!("ready")));

    let (invalid_plan, invalid_schemas) = plan(
        json!({"type": "string", "minLength": 3, "example": "x"}),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let invalid = BoundRequestBody::new(&invalid_plan, &invalid_schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));
    assert_eq!(invalid.synthesize().ok(), Some(json!("example")));
}

#[test]
fn terminates_optional_recursive_schemas() {
    let components = BTreeMap::from([(
        "Node".to_owned(),
        json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": {"type": "string"},
                "child": {"$ref": "#/components/schemas/Node"}
            }
        }),
    )]);
    let (plan, schemas) = plan(
        json!({"$ref": "#/components/schemas/Node"}),
        components,
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));
    let value = media.synthesize().unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(value, json!({"name": "example"}));
    assert!(media.validate(&value).is_empty());
}

#[test]
fn proves_ordered_date_times_are_sound() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["created_at", "updated_at"],
            "properties": {
                "created_at": {"type": "string", "format": "date-time"},
                "published_at": {"type": ["string", "null"], "format": "date-time"},
                "updated_at": {"type": "string", "format": "date-time"}
            },
            "x-simile-constraints": [
                {"ordered": {"paths": ["/created_at", "/published_at", "/updated_at"]}}
            ]
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    assert!(
        media
            .validate(&json!({
                "created_at": "2026-01-01T00:00:00Z",
                "published_at": "2026-01-02T00:00:00Z",
                "updated_at": "2026-01-03T00:00:00Z"
            }))
            .is_empty()
    );
    assert!(
        media
            .validate(&json!({
                "created_at": "2026-01-01T00:00:00Z",
                "published_at": null,
                "updated_at": "2026-01-01T00:00:00Z"
            }))
            .is_empty()
    );

    let issues = media.validate(&json!({
        "created_at": "2026-01-03T00:00:00Z",
        "published_at": "2026-01-02T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z"
    }));
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].path, "/published_at");
    assert_eq!(issues[0].rule, ValidationRule::Ordered);
    assert_eq!(issues[1].path, "/updated_at");
    assert_eq!(issues[1].rule, ValidationRule::Ordered);
}

#[test]
fn proves_nested_numeric_ordering_and_decodes_json_pointers() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["window"],
            "properties": {
                "window": {
                    "type": "object",
                    "required": ["starts~here", "ends/here"],
                    "properties": {
                        "starts~here": {"type": "integer"},
                        "ends/here": {"type": "number"}
                    }
                }
            },
            "x-simile-constraints": [
                {"ordered": {"paths": ["/window/starts~0here", "/window/ends~1here"]}}
            ]
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    assert!(
        media
            .validate(&json!({"window": {"starts~here": 4, "ends/here": 4.5}}))
            .is_empty()
    );
    let issues = media.validate(&json!({"window": {"starts~here": 5, "ends/here": 4.5}}));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].path, "/window/ends~1here");
    assert_eq!(issues[0].rule, ValidationRule::Ordered);
}

#[test]
fn fuzz_synthesis_is_proven_against_ordered_relationships() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["created_at", "published_at", "updated_at"],
            "properties": {
                "created_at": {"type": "string", "format": "date-time"},
                "published_at": {"type": "string", "format": "date-time"},
                "updated_at": {"type": "string", "format": "date-time"}
            },
            "x-simile-constraints": [
                {"ordered": {"paths": ["/created_at", "/published_at", "/updated_at"]}}
            ]
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    for seed in 0..64 {
        let value = media
            .synthesize_fuzz(seed)
            .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert!(
            media.validate(&value).is_empty(),
            "seed {seed} produced an unsound value: {value}"
        );
    }
}

#[test]
fn response_synthesis_is_proven_against_ordered_relationships() {
    let response = ResponseBodyContract {
        content: BTreeMap::from([(
            "application/json".to_owned(),
            MediaTypeContract {
                schema: Some(json!({
                    "type": "object",
                    "required": ["created_at", "published_at", "updated_at"],
                    "properties": {
                        "created_at": {"type": "string", "format": "date-time"},
                        "published_at": {"type": "string", "format": "date-time"},
                        "updated_at": {"type": "string", "format": "date-time"}
                    },
                    "x-simile-constraints": [
                        {"ordered": {"paths": ["/created_at", "/published_at", "/updated_at"]}}
                    ]
                })),
                example: None,
            },
        )]),
    };
    let components = BTreeMap::new();
    let mut compiler = BodySchemaCompiler::new(&components, OpenApiFeature::V3_1);
    let plan = compiler
        .compile_response(&response)
        .unwrap_or_else(|error| panic!("{error}"));
    let schemas = compiler.finish().unwrap_or_else(|error| panic!("{error}"));
    let media = BoundResponseBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    for seed in 0..64 {
        let value = media
            .synthesize_fuzz(seed)
            .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert!(
            media.validate(&value).is_empty(),
            "seed {seed} produced an unsound response: {value}"
        );
    }
}

#[test]
fn rejects_constraints_that_cannot_be_proven() {
    let unsupported = compile_error(json!({
        "type": "object",
        "x-simile-constraints": [{"implies": {}}]
    }));
    assert!(unsupported.contains("unsupported constraint kind `implies`"));

    let unknown_path = compile_error(json!({
        "type": "object",
        "properties": {"created_at": {"type": "string", "format": "date-time"}},
        "x-simile-constraints": [
            {"ordered": {"paths": ["/created_at", "/missing"]}}
        ]
    }));
    assert!(unknown_path.contains("does not resolve to a property"));

    let incompatible = compile_error(json!({
        "type": "object",
        "properties": {
            "created_at": {"type": "string", "format": "date-time"},
            "sequence": {"type": "integer"}
        },
        "x-simile-constraints": [
            {"ordered": {"paths": ["/created_at", "/sequence"]}}
        ]
    }));
    assert!(incompatible.contains("compatible value types"));
}

fn compile_error(schema: serde_json::Value) -> String {
    let body = RequestBodyContract {
        required: true,
        content: BTreeMap::from([(
            "application/json".to_owned(),
            MediaTypeContract {
                schema: Some(schema),
                example: None,
            },
        )]),
    };
    let components = BTreeMap::new();
    let mut compiler = BodySchemaCompiler::new(&components, OpenApiFeature::V3_1);
    match compiler
        .compile_request(&body)
        .and_then(|_| compiler.finish())
    {
        Err(error) => error.to_string(),
        Ok(_) => panic!("constraint should fail compilation"),
    }
}

fn plan(
    schema: serde_json::Value,
    components: BTreeMap<String, serde_json::Value>,
    feature: OpenApiFeature,
) -> (RequestBodyPlan, SchemaCatalog) {
    let body = RequestBodyContract {
        required: true,
        content: BTreeMap::from([(
            "application/json".to_owned(),
            MediaTypeContract {
                schema: Some(schema),
                example: None,
            },
        )]),
    };
    let mut compiler = BodySchemaCompiler::new(&components, feature);
    let plan = compiler
        .compile_request(&body)
        .unwrap_or_else(|error| panic!("{error}"));
    let schemas = compiler.finish().unwrap_or_else(|error| panic!("{error}"));
    (plan, schemas)
}
