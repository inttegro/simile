use std::collections::BTreeMap;

use serde_json::json;
use simile_heuristics::Policy;
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
fn compiles_validates_and_synthesizes_openapi_patterns() {
    let (id_plan, id_schemas) = plan(
        json!({"type": "string", "pattern": "^or_[a-z0-9]{16}$"}),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&id_plan, &id_schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    assert_eq!(
        media.validate(&json!("wrong"))[0].rule,
        ValidationRule::Pattern
    );
    for seed in 0..32 {
        let value = media
            .synthesize_fuzz(seed)
            .unwrap_or_else(|error| panic!("{error}"));
        let id = value
            .as_str()
            .unwrap_or_else(|| panic!("pattern should synthesize a string"));
        assert!(id.starts_with("or_"));
        assert_eq!(id.len(), 19);
        assert!(
            id[3..]
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        );
        assert!(media.validate(&value).is_empty());
    }

    let error = compile_error(json!({"type": "string", "pattern": "["}));
    assert!(error.contains("pattern"));

    // OpenAPI patterns use the ECMA-262 dialect, which includes lookarounds that Rust's native
    // regex dialect intentionally omits.
    let (plan, schemas) = plan(
        json!({"type": "string", "pattern": "^(?!bad$)[a-z]+$"}),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));
    assert!(media.validate(&json!("good")).is_empty());
    assert_eq!(
        media.validate(&json!("bad"))[0].rule,
        ValidationRule::Pattern
    );
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
                    examples: BTreeMap::new(),
                    request_relation: None,
                },
            ),
            (
                "*/*".to_owned(),
                MediaTypeContract {
                    schema: None,
                    example: None,
                    examples: BTreeMap::new(),
                    request_relation: None,
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
                examples: BTreeMap::new(),
                request_relation: None,
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
fn heuristics_repair_synthesis_without_becoming_validation_rules() {
    let schema = json!({
        "type": "object",
        "required": ["items", "item_count"],
        "properties": {
            "items": {
                "type": "array",
                "minItems": 2,
                "maxItems": 2,
                "items": {"type": "string"}
            },
            "item_count": {"type": "integer", "minimum": 0, "maximum": 20}
        }
    });
    let (standard_plan, standard_schemas) = plan_with_policy(schema.clone(), Policy::Standard);
    let (off_plan, off_schemas) = plan_with_policy(schema, Policy::Off);
    let standard = BoundRequestBody::new(&standard_plan, &standard_schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));
    let off = BoundRequestBody::new(&off_plan, &off_schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    for seed in 0..32 {
        let value = standard
            .synthesize_fuzz(seed)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(value["item_count"], 2);
    }
    assert!((0..32).any(|seed| {
        off.synthesize_fuzz(seed)
            .is_ok_and(|value| value["item_count"] != 2)
    }));

    assert!(
        standard
            .validate(&json!({"items": ["a", "b"], "item_count": 9}))
            .is_empty()
    );
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
                examples: BTreeMap::new(),
                request_relation: None,
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
                examples: BTreeMap::new(),
                request_relation: None,
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
                examples: BTreeMap::new(),
                request_relation: None,
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
fn synthesizes_a_union_branch_that_requires_a_sibling_property() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["id"],
            "oneOf": [
                {"required": ["product"]},
                {"required": ["bundle"]}
            ],
            "properties": {
                "id": {"type": "string"},
                "product": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id"],
                    "properties": {"id": {"type": "string"}}
                },
                "bundle": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id"],
                    "properties": {"id": {"type": "string"}}
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
    assert!(value.get("product").is_some() ^ value.get("bundle").is_some());
    assert!(media.validate(&value).is_empty(), "{value}");
}

#[test]
fn synthesizes_required_fields_from_a_nested_all_of_overlay() {
    let components = BTreeMap::from([
        (
            "Delivery".to_owned(),
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["kind", "sent_channels"],
                "properties": {
                    "deliveries": {"type": "array", "items": {"type": "string"}},
                    "kind": {"type": "string", "enum": ["invoice", "receipt"]},
                    "sent_channels": {"type": "array", "items": {"type": "string"}}
                }
            }),
        ),
        (
            "Response".to_owned(),
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["delivery"],
                "properties": {
                    "delivery": {"$ref": "#/components/schemas/Delivery"}
                }
            }),
        ),
    ]);
    let (plan, schemas) = plan(
        json!({
            "allOf": [
                {"$ref": "#/components/schemas/Response"},
                {
                    "type": "object",
                    "properties": {
                        "delivery": {
                            "allOf": [
                                {"$ref": "#/components/schemas/Delivery"},
                                {
                                    "type": "object",
                                    "required": ["deliveries"],
                                    "properties": {
                                        "kind": {"type": "string", "enum": ["invoice"]},
                                        "deliveries": {"type": "array", "minItems": 1}
                                    }
                                }
                            ]
                        }
                    }
                }
            ]
        }),
        components,
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    let value = media.synthesize().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(value["delivery"]["kind"], "invoice");
    assert_eq!(value["delivery"]["deliveries"], json!(["example"]));
    assert!(media.validate(&value).is_empty(), "{value}");
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
fn proves_implications_with_conjunctive_consequences() {
    let (plan, schemas) = plan(
        product_implication_schema(),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    assert!(
        media
            .validate(&json!({
                "active": false,
                "published_at": null,
                "archived_at": "2026-01-03T00:00:00Z",
                "checkout_enabled": false
            }))
            .is_empty()
    );
    assert!(
        media
            .validate(&json!({
                "active": true,
                "published_at": "2026-01-01T00:00:00Z",
                "archived_at": null,
                "checkout_enabled": true
            }))
            .is_empty()
    );

    let issues = media.validate(&json!({
        "active": false,
        "published_at": "2026-01-01T00:00:00Z",
        "archived_at": null,
        "checkout_enabled": true
    }));
    assert_eq!(issues.len(), 3);
    assert_eq!(issues[0].path, "/published_at");
    assert_eq!(issues[0].rule, ValidationRule::Implies);
    assert_eq!(issues[1].path, "/archived_at");
    assert_eq!(issues[1].rule, ValidationRule::Implies);
    assert_eq!(issues[2].path, "/checkout_enabled");
    assert_eq!(issues[2].rule, ValidationRule::Implies);
}

#[test]
fn implication_presence_distinguishes_missing_from_null() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["discounted"],
            "properties": {
                "coupon": {"type": ["string", "null"]},
                "discounted": {"type": "boolean"}
            },
            "x-simile-constraints": [{
                "implies": {
                    "when": {"path": "/coupon", "state": "present"},
                    "then": {"all": [
                        {"path": "/discounted", "equals": true}
                    ]}
                }
            }]
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    assert!(media.validate(&json!({"discounted": false})).is_empty());
    assert!(
        media
            .validate(&json!({"coupon": "SAVE", "discounted": true}))
            .is_empty()
    );
    let issues = media.validate(&json!({"coupon": null, "discounted": false}));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].path, "/discounted");
    assert_eq!(issues[0].rule, ValidationRule::Implies);
}

#[test]
fn implication_finds_a_non_null_witness_in_a_nullable_enum() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["active", "status"],
            "properties": {
                "active": {"type": "boolean"},
                "status": {"enum": [null, "ready"]}
            },
            "x-simile-constraints": [{
                "implies": {
                    "when": {"path": "/active", "equals": true},
                    "then": {"all": [
                        {"path": "/status", "state": "non-null"}
                    ]}
                }
            }]
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));
    let value = media.synthesize().unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(value["active"], true);
    assert_eq!(value["status"], "ready");
    assert!(media.validate(&value).is_empty());
}

#[test]
fn synthesis_establishes_every_triggered_consequence() {
    let (plan, schemas) = plan(
        product_implication_schema(),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));
    let mut saw_inactive = false;

    for seed in 0..64 {
        let value = media
            .synthesize_fuzz(seed)
            .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert!(
            media.validate(&value).is_empty(),
            "seed {seed} produced an unsound value: {value}"
        );
        if value["active"] == false {
            saw_inactive = true;
            assert!(value["published_at"].is_null());
            assert!(value["archived_at"].as_str().is_some());
            assert_eq!(value["checkout_enabled"], false);
        }
    }
    assert!(
        saw_inactive,
        "fuzz synthesis should exercise the implication"
    );
}

#[test]
fn synthesis_resolves_chained_implications_to_a_fixed_point() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["active", "status", "published_at"],
            "properties": {
                "active": {"type": "boolean"},
                "status": {"type": "string"},
                "published_at": {"type": ["string", "null"], "format": "date-time"}
            },
            "x-simile-constraints": [
                {
                    "implies": {
                        "when": {"path": "/status", "equals": "draft"},
                        "then": {"all": [
                            {"path": "/published_at", "state": "null"}
                        ]}
                    }
                },
                {
                    "implies": {
                        "when": {"path": "/active", "equals": true},
                        "then": {"all": [
                            {"path": "/status", "equals": "draft"}
                        ]}
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

    assert_eq!(value["active"], true);
    assert_eq!(value["status"], "draft");
    assert!(value["published_at"].is_null());
    assert!(media.validate(&value).is_empty());
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
                examples: BTreeMap::new(),
                request_relation: None,
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
fn response_synthesis_is_proven_against_derived_relationships() {
    let response = ResponseBodyContract {
        content: BTreeMap::from([(
            "application/json".to_owned(),
            MediaTypeContract {
                schema: Some(json!({
                    "type": "object",
                    "required": ["id", "url", "subtotal", "lines"],
                    "properties": {
                        "id": {"type": "string"},
                        "url": {"type": "string"},
                        "subtotal": {"type": "integer"},
                        "lines": {
                            "type": "array",
                            "minItems": 1,
                            "items": {
                                "type": "object",
                                "required": ["amount"],
                                "properties": {"amount": {"type": "integer"}}
                            }
                        }
                    },
                    "x-simile-constraints": [
                        {
                            "aggregate": {
                                "target": "/subtotal",
                                "operation": "sum",
                                "source": "/lines/*/amount"
                            }
                        },
                        {
                            "template": {
                                "path": "/url",
                                "value": "/v1/invoices/{/id}/lines"
                            }
                        }
                    ]
                })),
                example: None,
                examples: BTreeMap::new(),
                request_relation: None,
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

    for seed in 0..32 {
        let value = media
            .synthesize_fuzz(seed)
            .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert!(media.validate(&value).is_empty(), "seed {seed}: {value}");
    }
}

#[test]
fn proves_equal_values_across_array_items() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["currency", "lines"],
            "properties": {
                "currency": {"type": "string", "enum": ["usd", "gbp"]},
                "lines": {
                    "type": "array",
                    "minItems": 2,
                    "items": {
                        "type": "object",
                        "required": ["currency"],
                        "properties": {
                            "currency": {"type": "string", "enum": ["usd", "gbp"]}
                        }
                    }
                }
            },
            "x-simile-constraints": [{
                "equal": {"paths": ["/currency", "/lines/*/currency"]}
            }]
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
                "currency": "usd",
                "lines": [{"currency": "usd"}, {"currency": "usd"}]
            }))
            .is_empty()
    );
    let issues = media.validate(&json!({
        "currency": "usd",
        "lines": [{"currency": "gbp"}, {"currency": "usd"}]
    }));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].path, "/lines/0/currency");
    assert_eq!(issues[0].rule, ValidationRule::Equal);

    for seed in 0..32 {
        let value = media
            .synthesize_fuzz(seed)
            .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert!(media.validate(&value).is_empty(), "seed {seed}: {value}");
    }
}

#[test]
fn proves_numeric_equations_and_constructs_the_target() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["amount_due", "amount_paid", "amount_remaining"],
            "properties": {
                "amount_due": {"type": "integer", "minimum": 0},
                "amount_paid": {"type": "integer", "minimum": 0},
                "amount_remaining": {"type": "integer"}
            },
            "x-simile-constraints": [{
                "equation": {
                    "target": "/amount_remaining",
                    "value": {"subtract": [
                        {"path": "/amount_due"},
                        {"path": "/amount_paid"}
                    ]}
                }
            }]
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
                "amount_due": 30,
                "amount_paid": 12,
                "amount_remaining": 18
            }))
            .is_empty()
    );
    let issues = media.validate(&json!({
        "amount_due": 30,
        "amount_paid": 12,
        "amount_remaining": 14
    }));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].rule, ValidationRule::Equation);

    for seed in 0..32 {
        let value = media
            .synthesize_fuzz(seed)
            .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert!(media.validate(&value).is_empty(), "seed {seed}: {value}");
    }
}

#[test]
fn proves_guarded_array_aggregates() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["subtotal", "lines"],
            "properties": {
                "subtotal": {"type": "integer"},
                "lines": {
                    "type": "object",
                    "required": ["has_more", "data"],
                    "properties": {
                        "has_more": {"type": "boolean"},
                        "data": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "required": ["amount"],
                                "properties": {"amount": {"type": "integer"}}
                            }
                        }
                    }
                }
            },
            "x-simile-constraints": [{
                "aggregate": {
                    "target": "/subtotal",
                    "operation": "sum",
                    "source": "/lines/data/*/amount",
                    "when": {"path": "/lines/has_more", "equals": false}
                }
            }]
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
                "subtotal": 12,
                "lines": {"has_more": false, "data": [{"amount": 5}, {"amount": 7}]}
            }))
            .is_empty()
    );
    assert!(
        media
            .validate(&json!({
                "subtotal": 999,
                "lines": {"has_more": true, "data": [{"amount": 5}]}
            }))
            .is_empty()
    );
    let issues = media.validate(&json!({
        "subtotal": 11,
        "lines": {"has_more": false, "data": [{"amount": 5}, {"amount": 7}]}
    }));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].rule, ValidationRule::Aggregate);
}

#[test]
fn proves_field_templates() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["id", "url"],
            "properties": {
                "id": {"type": "string"},
                "url": {"type": "string"}
            },
            "x-simile-constraints": [{
                "template": {
                    "path": "/url",
                    "value": "/v1/invoices/{/id}/lines"
                }
            }]
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    assert!(
        media
            .validate(&json!({"id": "in_123", "url": "/v1/invoices/in_123/lines"}))
            .is_empty()
    );
    let issues = media.validate(&json!({"id": "in_123", "url": "wrong"}));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].rule, ValidationRule::Template);

    let value = media.synthesize().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        value["url"],
        format!("/v1/invoices/{}/lines", value["id"].as_str().unwrap_or(""))
    );
    assert!(media.validate(&value).is_empty());
}

#[test]
fn implication_supports_contains_and_path_comparisons() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["amount_due", "amount_paid", "overpaid", "methods", "twint_enabled"],
            "properties": {
                "amount_due": {"type": "integer"},
                "amount_paid": {"type": "integer"},
                "overpaid": {"type": "boolean"},
                "methods": {"type": "array", "items": {"type": "string"}},
                "twint_enabled": {"type": "boolean"}
            },
            "x-simile-constraints": [
                {
                    "implies": {
                        "when": {
                            "compare": {
                                "left": {"path": "/amount_paid"},
                                "operator": "greater-than",
                                "right": {"path": "/amount_due"}
                            }
                        },
                        "then": {"all": [{"path": "/overpaid", "equals": true}]}
                    }
                },
                {
                    "implies": {
                        "when": {"path": "/methods", "contains": "twint"},
                        "then": {"all": [{"path": "/twint_enabled", "equals": true}]}
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
    let issues = media.validate(&json!({
        "amount_due": 10,
        "amount_paid": 12,
        "overpaid": false,
        "methods": ["twint"],
        "twint_enabled": false
    }));
    assert_eq!(issues.len(), 2);
    assert!(
        issues
            .iter()
            .all(|issue| issue.rule == ValidationRule::Implies)
    );
}

#[test]
fn implication_combines_multiple_membership_consequences() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["active", "capabilities"],
            "properties": {
                "active": {"const": true},
                "capabilities": {
                    "type": "array",
                    "minItems": 2,
                    "uniqueItems": true,
                    "items": {"type": "string", "enum": ["payments", "refunds"]}
                }
            },
            "x-simile-constraints": [{
                "implies": {
                    "when": {"path": "/active", "equals": true},
                    "then": {"all": [
                        {"path": "/capabilities", "contains": "payments"},
                        {"path": "/capabilities", "contains": "refunds"}
                    ]}
                }
            }]
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));
    let value = media.synthesize().unwrap_or_else(|error| panic!("{error}"));

    assert!(media.validate(&value).is_empty());
    assert_eq!(value["capabilities"], json!(["payments", "refunds"]));
}

#[test]
fn proves_lifecycle_paths_and_clears_incompatible_transitions() {
    let (plan, schemas) = plan(
        invoice_lifecycle_schema(),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    assert!(
        media
            .validate(&json!({
                "status": "open",
                "created": 1,
                "status_transitions": {
                    "finalized_at": 2,
                    "paid_at": null,
                    "marked_uncollectible_at": null,
                    "voided_at": null
                }
            }))
            .is_empty()
    );
    let issues = media.validate(&json!({
        "status": "paid",
        "created": 1,
        "status_transitions": {
            "finalized_at": null,
            "paid_at": 3,
            "marked_uncollectible_at": null,
            "voided_at": 2
        }
    }));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].rule, ValidationRule::Lifecycle);

    for seed in 0..64 {
        let value = media
            .synthesize_fuzz(seed)
            .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert!(media.validate(&value).is_empty(), "seed {seed}: {value}");
    }
}

#[test]
fn proves_and_constructs_relative_bounds() {
    let (bounded_plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["lower", "value", "upper"],
            "properties": {
                "lower": {"type": "integer", "const": 1},
                "value": {"type": "integer", "minimum": 0, "maximum": 10},
                "upper": {"type": "integer", "const": 5}
            },
            "x-simile-constraints": [{
                "bounded": {
                    "path": "/value",
                    "exclusive-minimum": {"path": "/lower"},
                    "exclusive-maximum": {"path": "/upper"}
                }
            }]
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&bounded_plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    assert!(
        media
            .validate(&json!({"lower": 1, "value": 3, "upper": 5}))
            .is_empty()
    );
    for value in [1, 5] {
        let issues = media.validate(&json!({"lower": 1, "value": value, "upper": 5}));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].rule, ValidationRule::Bounded);
    }
    for seed in 0..16 {
        let value = media
            .synthesize_fuzz(seed)
            .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert!(media.validate(&value).is_empty(), "seed {seed}: {value}");
    }

    let (optional_plan, optional_schemas) = plan(
        json!({
            "type": "object",
            "required": ["lower"],
            "properties": {
                "lower": {"type": "integer", "const": 4},
                "value": {"type": ["integer", "null"]}
            },
            "x-simile-constraints": [{
                "bounded": {
                    "path": "/value",
                    "minimum": {"path": "/lower"}
                }
            }]
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let optional = BoundRequestBody::new(&optional_plan, &optional_schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));
    assert!(optional.validate(&json!({"lower": 4})).is_empty());
    assert!(
        optional
            .validate(&json!({"lower": 4, "value": null}))
            .is_empty()
    );
    for seed in 0..16 {
        let value = optional
            .synthesize_fuzz_minimal(seed)
            .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert!(value.get("value").is_none(), "seed {seed}: {value}");
        assert!(optional.validate(&value).is_empty(), "seed {seed}: {value}");
    }
}

#[test]
fn minimal_synthesis_keeps_optional_aggregate_targets_absent() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["values"],
            "properties": {
                "values": {
                    "type": "array",
                    "items": {"type": "integer"}
                },
                "sum": {"type": ["integer", "null"]}
            },
            "x-simile-constraints": [{
                "aggregate": {
                    "target": "/sum",
                    "operation": "sum",
                    "source": "/values/*"
                }
            }]
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    assert!(media.validate(&json!({"values": []})).is_empty());
    assert!(
        media
            .validate(&json!({"values": [], "sum": null}))
            .is_empty()
    );
    for seed in 0..16 {
        let value = media
            .synthesize_fuzz_minimal(seed)
            .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert_eq!(value["values"], json!([]), "seed {seed}: {value}");
        assert!(value.get("sum").is_none(), "seed {seed}: {value}");
        assert!(media.validate(&value).is_empty(), "seed {seed}: {value}");
    }
}

#[test]
fn minimal_synthesis_can_avoid_an_optional_array_item_union() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["size", "purchase_intents"],
            "properties": {
                "size": {"type": "integer", "minimum": 0},
                "purchase_intents": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "product": {"type": "object"},
                            "bundle": {"type": "object"}
                        },
                        "oneOf": [
                            {"required": ["product"]},
                            {"required": ["bundle"]}
                        ],
                        "x-simile-constraints": [
                            {
                                "implies": {
                                    "when": {"path": "/product", "state": "present"},
                                    "then": {"all": [{"path": "/bundle", "state": "absent"}]}
                                }
                            },
                            {
                                "implies": {
                                    "when": {"path": "/bundle", "state": "present"},
                                    "then": {"all": [{"path": "/product", "state": "absent"}]}
                                }
                            }
                        ]
                    }
                }
            },
            "x-simile-constraints": [{
                "aggregate": {
                    "target": "/size",
                    "operation": "count",
                    "source": "/purchase_intents/*"
                }
            }]
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    let value = media
        .synthesize_fuzz_minimal(1)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(value, json!({"size": 0, "purchase_intents": []}));
    assert!(media.validate(&value).is_empty());
}

#[test]
fn all_of_overlays_reestablish_inherited_constraints() {
    let (plan, schemas) = plan(
        json!({
            "allOf": [
                {
                    "type": "object",
                    "required": ["status", "initiated_at"],
                    "properties": {
                        "status": {"type": "string", "enum": ["scheduled", "canceled"]},
                        "initiated_at": {"type": "string", "format": "date-time"},
                        "canceled_at": {"type": "string", "format": "date-time"},
                        "cancellation": {
                            "type": "object",
                            "required": ["reason"],
                            "properties": {"reason": {"type": "string"}}
                        }
                    },
                    "x-simile-constraints": [
                        {"ordered": {"paths": ["/initiated_at", "/canceled_at"]}},
                        {
                            "implies": {
                                "when": {"path": "/status", "equals": "canceled"},
                                "then": {"all": [
                                    {"path": "/canceled_at", "state": "non-null"},
                                    {"path": "/cancellation", "state": "non-null"}
                                ]}
                            }
                        }
                    ]
                },
                {
                    "type": "object",
                    "required": ["canceled_at", "cancellation"],
                    "properties": {
                        "status": {"type": "string", "enum": ["canceled"]}
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

    for seed in 0..16 {
        let value = media
            .synthesize_fuzz_minimal(seed)
            .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert_eq!(value["status"], "canceled", "seed {seed}: {value}");
        assert!(media.validate(&value).is_empty(), "seed {seed}: {value}");
    }
}

#[test]
fn synthesis_exhaustion_reports_the_closest_branch_failure() {
    let (plan, schemas) = plan(
        json!({
            "type": "string",
            "oneOf": [
                {"type": "string"},
                {"type": "string"}
            ]
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    let error = match media.synthesize_fuzz_minimal(1) {
        Ok(value) => panic!("identical oneOf branches synthesized an invalid value: {value}"),
        Err(error) => error,
    };
    let message = error.to_string();
    assert!(message.contains("closest candidate failed"), "{message}");
    assert!(message.contains("`/` (oneOf)"), "{message}");
    assert!(message.contains("matched 2"), "{message}");
}

#[test]
fn evaluates_extended_aggregates() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["values", "count", "sum", "minimum", "maximum"],
            "properties": {
                "values": {
                    "type": "array",
                    "minItems": 1,
                    "items": {"type": "integer", "minimum": 1, "maximum": 9}
                },
                "count": {"type": "integer"},
                "sum": {"type": "integer"},
                "minimum": {"type": "integer"},
                "maximum": {"type": "integer"}
            },
            "x-simile-constraints": [
                {"aggregate": {"target": "/count", "operation": "count", "source": "/values/*"}},
                {"aggregate": {"target": "/sum", "operation": "sum", "source": "/values/*"}},
                {"aggregate": {"target": "/minimum", "operation": "min", "source": "/values/*"}},
                {"aggregate": {"target": "/maximum", "operation": "max", "source": "/values/*"}}
            ]
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    let sound = json!({
        "values": [4, 2, 7],
        "count": 3,
        "sum": 13,
        "minimum": 2,
        "maximum": 7
    });
    assert!(media.validate(&sound).is_empty());
    let issues = media.validate(&json!({
        "values": [4, 2, 7],
        "count": 2,
        "sum": 12,
        "minimum": 4,
        "maximum": 4
    }));
    assert_eq!(issues.len(), 4);
    assert!(
        issues
            .iter()
            .all(|issue| issue.rule == ValidationRule::Aggregate)
    );

    for seed in 0..16 {
        let value = media
            .synthesize_fuzz(seed)
            .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert!(media.validate(&value).is_empty(), "seed {seed}: {value}");
    }
}

#[test]
fn proves_distinct_selected_values() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["lines"],
            "properties": {
                "lines": {
                    "type": "array",
                    "minItems": 3,
                    "items": {
                        "type": "object",
                        "required": ["id"],
                        "properties": {
                            "id": {"type": "string", "enum": ["line_a", "line_b", "line_c"]}
                        }
                    }
                }
            },
            "x-simile-constraints": [{
                "distinct": {"source": "/lines/*/id"}
            }]
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    let issues = media.validate(&json!({
        "lines": [{"id": "line_a"}, {"id": "line_a"}, {"id": "line_b"}]
    }));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].path, "/lines/1/id");
    assert_eq!(issues[0].rule, ValidationRule::Distinct);

    for seed in 0..16 {
        let value = media
            .synthesize_fuzz(seed)
            .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert!(media.validate(&value).is_empty(), "seed {seed}: {value}");
    }
}

#[test]
fn proves_and_constructs_set_relationships() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["enabled", "supported", "blocked"],
            "properties": {
                "enabled": {
                    "type": "array",
                    "items": {"type": "string", "enum": ["card", "bank", "wallet"]}
                },
                "supported": {
                    "type": "array",
                    "minItems": 1,
                    "items": {"type": "string", "enum": ["card", "bank", "wallet"]}
                },
                "blocked": {
                    "type": "array",
                    "items": {"type": "string", "enum": ["card", "bank", "wallet"]}
                }
            },
            "x-simile-constraints": [
                {"set": {"left": "/enabled", "relation": "subset-of", "right": "/supported"}},
                {"set": {"left": "/blocked", "relation": "disjoint", "right": "/enabled"}}
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
                "enabled": ["card"],
                "supported": ["card", "bank"],
                "blocked": ["wallet"]
            }))
            .is_empty()
    );
    let issues = media.validate(&json!({
        "enabled": ["wallet"],
        "supported": ["card", "bank"],
        "blocked": ["wallet"]
    }));
    assert_eq!(issues.len(), 2);
    assert!(issues.iter().all(|issue| issue.rule == ValidationRule::Set));

    for seed in 0..16 {
        let value = media
            .synthesize_fuzz(seed)
            .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert!(media.validate(&value).is_empty(), "seed {seed}: {value}");
    }
}

#[test]
fn applies_equations_inside_each_array_item() {
    let (plan, schemas) = plan(
        json!({
            "type": "object",
            "required": ["lines"],
            "properties": {
                "lines": {
                    "type": "array",
                    "minItems": 2,
                    "items": {
                        "type": "object",
                        "required": ["quantity", "unit_amount", "amount"],
                        "properties": {
                            "quantity": {"type": "integer", "minimum": 1, "maximum": 5},
                            "unit_amount": {"type": "integer", "minimum": 1, "maximum": 100},
                            "amount": {"type": "integer"}
                        },
                        "x-simile-constraints": [{
                            "equation": {
                                "target": "/amount",
                                "value": {
                                    "add": [
                                        {"multiply": [
                                            {"path": "/quantity"},
                                            {"path": "/unit_amount"}
                                        ]},
                                        {"literal": 2}
                                    ]
                                }
                            }
                        }]
                    }
                }
            }
        }),
        BTreeMap::new(),
        OpenApiFeature::V3_1,
    );
    let media = BoundRequestBody::new(&plan, &schemas)
        .select("application/json")
        .unwrap_or_else(|| panic!("media type should be present"));

    let issues = media.validate(&json!({
        "lines": [
            {"quantity": 2, "unit_amount": 5, "amount": 12},
            {"quantity": 3, "unit_amount": 7, "amount": 20}
        ]
    }));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].path, "/lines/1/amount");
    assert_eq!(issues[0].rule, ValidationRule::Equation);

    for seed in 0..16 {
        let value = media
            .synthesize_fuzz(seed)
            .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert!(media.validate(&value).is_empty(), "seed {seed}: {value}");
    }
}

#[test]
fn rejects_constraints_that_cannot_be_proven() {
    let root_path = compile_error(json!({
        "type": "object",
        "properties": {"active": {"type": "boolean"}},
        "x-simile-constraints": [{
            "implies": {
                "when": {"path": "", "state": "present"},
                "then": {"all": [
                    {"path": "/active", "equals": true}
                ]}
            }
        }]
    }));
    assert!(root_path.contains("path must be a non-empty absolute JSON Pointer"));

    let unsupported_branch = compile_error(json!({
        "type": "object",
        "properties": {"active": {"type": "boolean"}},
        "x-simile-constraints": [{
            "implies": {
                "when": {"path": "/active", "equals": false},
                "then": {"any": []}
            }
        }]
    }));
    assert!(unsupported_branch.contains("then currently supports exactly one `all` array"));

    let conflicting = compile_error(json!({
        "type": "object",
        "required": ["active", "published_at"],
        "properties": {
            "active": {"type": "boolean"},
            "published_at": {"type": ["string", "null"], "format": "date-time"}
        },
        "x-simile-constraints": [{
            "implies": {
                "when": {"path": "/active", "equals": false},
                "then": {"all": [
                    {"path": "/published_at", "state": "null"},
                    {"path": "/published_at", "state": "non-null"}
                ]}
            }
        }]
    }));
    assert!(conflicting.contains("assertions for the same path cannot all be satisfied"));

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

    let optional_equation_source = compile_error(json!({
        "type": "object",
        "required": ["total"],
        "properties": {
            "subtotal": {"type": "integer"},
            "total": {"type": "integer"}
        },
        "x-simile-constraints": [{
            "equation": {
                "target": "/total",
                "value": {"add": [{"path": "/subtotal"}, {"path": "/subtotal"}]}
            }
        }]
    }));
    assert!(optional_equation_source.contains("must be required"));

    let optional_bound_source = compile_error(json!({
        "type": "object",
        "properties": {
            "lower": {"type": "integer"},
            "value": {"type": "integer"}
        },
        "x-simile-constraints": [{
            "bounded": {
                "path": "/value",
                "minimum": {"path": "/lower"}
            }
        }]
    }));
    assert!(optional_bound_source.contains("must be required"));

    let scalar_aggregate = compile_error(json!({
        "type": "object",
        "required": ["amount", "total"],
        "properties": {
            "amount": {"type": "integer"},
            "total": {"type": "integer"}
        },
        "x-simile-constraints": [{
            "aggregate": {
                "target": "/total",
                "operation": "sum",
                "source": "/amount"
            }
        }]
    }));
    assert!(scalar_aggregate.contains("array wildcard"));

    let scalar_distinct = compile_error(json!({
        "type": "object",
        "properties": {"id": {"type": "string"}},
        "x-simile-constraints": [{
            "distinct": {"source": "/id"}
        }]
    }));
    assert!(scalar_distinct.contains("array wildcard"));

    let optional_set_reference = compile_error(json!({
        "type": "object",
        "required": ["enabled"],
        "properties": {
            "enabled": {"type": "array", "items": {"type": "string"}},
            "supported": {"type": "array", "items": {"type": "string"}}
        },
        "x-simile-constraints": [{
            "set": {
                "left": "/enabled",
                "relation": "subset-of",
                "right": "/supported"
            }
        }]
    }));
    assert!(optional_set_reference.contains("right-hand set must be required"));

    let incompatible_equal = compile_error(json!({
        "type": "object",
        "properties": {
            "amount": {"type": "integer"},
            "currency": {"type": "string"}
        },
        "x-simile-constraints": [{
            "equal": {"paths": ["/amount", "/currency"]}
        }]
    }));
    assert!(incompatible_equal.contains("compatible types"));

    let cyclic_lifecycle = compile_error(json!({
        "type": "object",
        "required": ["status", "created"],
        "properties": {
            "status": {"type": "string", "enum": ["draft", "open"]},
            "created": {"type": "integer"},
            "opened_at": {"type": ["integer", "null"]},
            "reopened_at": {"type": ["integer", "null"]}
        },
        "x-simile-constraints": [{
            "lifecycle": {
                "state": "/status",
                "created-at": "/created",
                "transitions": [
                    {"from": "draft", "to": "open", "at": "/opened_at"},
                    {"from": "open", "to": "draft", "at": "/reopened_at"}
                ]
            }
        }]
    }));
    assert!(cyclic_lifecycle.contains("must not contain cycles"));
}

fn product_implication_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "required": ["active", "published_at", "archived_at", "checkout_enabled"],
        "properties": {
            "active": {"type": "boolean"},
            "published_at": {"type": ["string", "null"], "format": "date-time"},
            "archived_at": {"type": ["string", "null"], "format": "date-time"},
            "checkout_enabled": {"type": "boolean"}
        },
        "x-simile-constraints": [
            {
                "implies": {
                    "when": {"path": "/active", "equals": false},
                    "then": {"all": [
                        {"path": "/published_at", "state": "null-or-absent"},
                        {"path": "/archived_at", "state": "non-null"},
                        {"path": "/checkout_enabled", "equals": false}
                    ]}
                }
            }
        ]
    })
}

fn invoice_lifecycle_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "required": ["status", "created", "status_transitions"],
        "properties": {
            "status": {"type": "string", "enum": ["draft", "open", "paid", "uncollectible", "void"]},
            "created": {"type": "integer"},
            "status_transitions": {
                "type": "object",
                "required": ["finalized_at", "paid_at", "marked_uncollectible_at", "voided_at"],
                "properties": {
                    "finalized_at": {"type": ["integer", "null"]},
                    "paid_at": {"type": ["integer", "null"]},
                    "marked_uncollectible_at": {"type": ["integer", "null"]},
                    "voided_at": {"type": ["integer", "null"]}
                }
            }
        },
        "x-simile-constraints": [{
            "lifecycle": {
                "state": "/status",
                "created-at": "/created",
                "transitions": [
                    {"from": "draft", "to": "open", "at": "/status_transitions/finalized_at"},
                    {"from": ["open", "uncollectible"], "to": "paid", "at": "/status_transitions/paid_at"},
                    {"from": "open", "to": "uncollectible", "at": "/status_transitions/marked_uncollectible_at"},
                    {"from": ["open", "uncollectible"], "to": "void", "at": "/status_transitions/voided_at"}
                ]
            }
        }]
    })
}

fn compile_error(schema: serde_json::Value) -> String {
    let body = RequestBodyContract {
        required: true,
        content: BTreeMap::from([(
            "application/json".to_owned(),
            MediaTypeContract {
                schema: Some(schema),
                example: None,
                examples: BTreeMap::new(),
                request_relation: None,
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
                examples: BTreeMap::new(),
                request_relation: None,
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

fn plan_with_policy(schema: serde_json::Value, policy: Policy) -> (RequestBodyPlan, SchemaCatalog) {
    let body = RequestBodyContract {
        required: true,
        content: BTreeMap::from([(
            "application/json".to_owned(),
            MediaTypeContract {
                schema: Some(schema),
                example: None,
                examples: BTreeMap::new(),
                request_relation: None,
            },
        )]),
    };
    let components = BTreeMap::new();
    let mut compiler =
        BodySchemaCompiler::with_heuristics(&components, OpenApiFeature::V3_1, policy);
    let plan = compiler
        .compile_request(&body)
        .unwrap_or_else(|error| panic!("{error}"));
    let schemas = compiler.finish().unwrap_or_else(|error| panic!("{error}"));
    (plan, schemas)
}
