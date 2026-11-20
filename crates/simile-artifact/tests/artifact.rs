use std::collections::BTreeMap;

use serde_json::json;
use simile_artifact::{Error, FORMAT_VERSION, decode, encode};
use simile_spec::{
    ApiContract, AsyncOperation, Behavior, BehaviorOrigin, CompiledOperation, CompiledService,
    DescriptionCatalog, DescriptionEntry, DescriptionKind, HttpMethod, MediaTypeContract,
    OpenApiFeature, OpenApiVersion, Operation, OperationKey, Outcome, OutcomeProvenance,
    RequestBodyContract, Resource, ResourceKind, ResponseBodyContract, ResponseContract,
    ResponseStatus, ServiceInfo,
};

#[test]
fn encoding_is_deterministic_and_round_trips_metadata() {
    let service = example_service();
    let first = encode(&service).unwrap_or_else(|error| panic!("{error}"));
    let second = encode(&service).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(first, second);

    let artifact = decode(&first).unwrap_or_else(|error| panic!("{error}"));
    let summary = artifact.summary();
    assert_eq!(summary.format_version, FORMAT_VERSION);
    assert_eq!(summary.parameters, 0);
    assert_eq!(summary.service, "Orders");
    assert_eq!(summary.service_version, "2026-11-15");
    assert_eq!(summary.openapi_version, "3.2.1");
    assert_eq!(
        summary.heuristic_policy,
        simile_heuristics::Policy::Standard
    );
    assert_eq!(summary.operations, 1);
    assert_eq!(summary.request_bodies, 1);
    assert_eq!(summary.response_bodies, 1);
    assert!(summary.schema_nodes > 0);
    assert_eq!(summary.responses, 1);
    assert_eq!(summary.resources, 1);
    assert_eq!(summary.async_operations, 1);
    assert_eq!(summary.effects, 0);
    assert_eq!(
        summary.webhook_policy,
        simile_spec::WebhookPolicy::default()
    );
    assert_eq!(summary.provider_authored_effects, 0);
    assert_eq!(summary.convention_inferred_effects, 0);
    assert_eq!(summary.effect_actions, 0);
    assert_eq!(summary.faults, 0);
    assert_eq!(summary.fault_profiles, 0);
    assert_eq!(summary.encoded_bytes, first.len());
    assert_eq!(summary.payload_sha256.len(), 64);

    let operation = artifact
        .operation_by_id("createOrder")
        .unwrap_or_else(|| panic!("operationId should be indexed"));
    assert_eq!(operation.key(), "POST /orders");
    assert_eq!(operation.operation_id(), Some("createOrder"));
    assert!(artifact.operation_by_id("CreateOrder").is_none());
    assert_eq!(
        artifact.description(DescriptionKind::Behavior, None, "createOrder"),
        Some("Create an order for one customer.")
    );

    let request = operation
        .request_body()
        .and_then(|body| body.select("application/json"))
        .unwrap_or_else(|| panic!("request body should be compiled into the artifact"));
    assert!(
        request
            .validate(&json!({
                "customerId": "cus_1",
                "startsAt": 1,
                "endsAt": 2,
                "lower": 0,
                "upper": 10,
                "amount": 5,
                "quantity": 2,
                "unitAmount": 3,
                "total": 8,
                "lineCount": 2,
                "lineAmounts": [4, 7],
                "lineMinimum": 4,
                "lineMaximum": 7,
                "lineIds": ["a", "b"],
                "enabled": ["card"],
                "supported": ["card", "bank"]
            }))
            .is_empty()
    );
    assert_eq!(
        request.validate(&json!({
            "customerId": "cus_1",
            "startsAt": 2,
            "endsAt": 1,
            "lower": 0,
            "upper": 10,
            "amount": 5,
            "quantity": 2,
            "unitAmount": 3,
            "total": 8,
            "lineCount": 2,
            "lineAmounts": [4, 7],
            "lineMinimum": 4,
            "lineMaximum": 7,
            "lineIds": ["a", "b"],
            "enabled": ["card"],
            "supported": ["card", "bank"]
        }))[0]
            .rule
            .to_string(),
        "ordered"
    );
    let conditional = request.validate(&json!({
        "customerId": "cus_1",
        "lower": 0,
        "upper": 10,
        "amount": 5,
        "quantity": 2,
        "unitAmount": 3,
        "total": 8,
        "lineCount": 2,
        "lineAmounts": [4, 7],
        "lineMinimum": 4,
        "lineMaximum": 7,
        "lineIds": ["a", "b"],
        "enabled": ["card"],
        "supported": ["card", "bank"],
        "active": false,
        "publishedAt": "2026-01-01T00:00:00Z",
        "archivedAt": null,
        "checkoutEnabled": true
    }));
    assert_eq!(conditional.len(), 3);
    assert!(
        conditional
            .iter()
            .all(|issue| issue.rule.to_string() == "implies")
    );
}

#[test]
fn response_source_order_does_not_change_the_artifact() {
    let mut service = example_service();
    let key = service
        .operations
        .keys()
        .next()
        .cloned()
        .unwrap_or_else(|| {
            panic!("example service should contain an operation");
        });
    let operation = service
        .operations
        .get_mut(&key)
        .unwrap_or_else(|| panic!("example operation should exist"));
    operation.contract.responses.push(ResponseContract {
        status: ResponseStatus::Exact(400),
        definition: json!({"description": "Bad request"}),
        headers: Vec::new(),
        body: None,
    });
    service
        .contract
        .operations
        .get_mut(&key)
        .unwrap_or_else(|| panic!("example contract operation should exist"))
        .responses = operation.contract.responses.clone();

    let ordered = encode(&service).unwrap_or_else(|error| panic!("{error}"));
    service
        .operations
        .get_mut(&key)
        .unwrap_or_else(|| panic!("example operation should exist"))
        .contract
        .responses
        .reverse();
    service
        .contract
        .operations
        .get_mut(&key)
        .unwrap_or_else(|| panic!("example contract operation should exist"))
        .responses
        .reverse();
    let reversed = encode(&service).unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(ordered, reversed);
}

#[test]
fn rejects_changed_payload_bytes() {
    let mut encoded = encode(&example_service()).unwrap_or_else(|error| panic!("{error}"));
    let last = encoded.len() - 1;
    encoded[last] ^= 1;
    assert!(matches!(decode(&encoded), Err(Error::Integrity)));
}

#[test]
fn rejects_wrong_format_and_trailing_bytes() {
    let encoded = encode(&example_service()).unwrap_or_else(|error| panic!("{error}"));
    let mut wrong_format = encoded.clone();
    wrong_format[8..10].copy_from_slice(&1_u16.to_be_bytes());
    assert!(matches!(
        decode(&wrong_format),
        Err(Error::UnsupportedFormat(1))
    ));

    let mut trailing = encoded;
    trailing.push(0);
    assert!(matches!(decode(&trailing), Err(Error::TrailingBytes)));
}

#[test]
fn rejects_provider_outcome_body_that_violates_the_declared_response() {
    let mut service = example_service();
    let operation = service
        .operations
        .values_mut()
        .next()
        .unwrap_or_else(|| panic!("example operation should exist"));
    operation.outcomes.push(Outcome {
        name: "created".to_owned(),
        status: 201,
        content_type: Some("application/json".to_owned()),
        body: Some(json!({"customerId": "cus_1"})),
        provenance: OutcomeProvenance::ProviderAuthored,
        when: Vec::new(),
    });

    assert!(matches!(
        encode(&service),
        Err(Error::InvalidOutcomeBody { outcome, .. }) if outcome == "created"
    ));
}

#[test]
fn rejects_private_implementation_markers_from_public_artifacts() {
    let mut service = example_service();
    let operation = service
        .operations
        .values_mut()
        .next()
        .unwrap_or_else(|| panic!("example operation should exist"));
    operation.outcomes.push(Outcome {
        name: "_internal_fraud_threshold".to_owned(),
        status: 201,
        content_type: Some("application/json".to_owned()),
        body: Some(json!({"customerId": "cus_1", "id": "ord_1"})),
        provenance: OutcomeProvenance::ProviderAuthored,
        when: Vec::new(),
    });

    assert!(matches!(
        encode(&service),
        Err(Error::UnsafePublicExport(marker)) if marker == "_internal_fraud_threshold"
    ));
}

fn example_service() -> CompiledService {
    let method = HttpMethod::Post;
    let key = OperationKey::new(&method, "/orders");
    let operation = Operation {
        key: key.clone(),
        operation_id: Some("createOrder".to_owned()),
        method,
        path: "/orders".to_owned(),
        summary: Some("Create an order".to_owned()),
        tags: vec!["orders".to_owned()],
        parameters: Vec::new(),
        request_body: Some(json!({"required": true, "schema": {"type": "object"}})),
        request: Some(RequestBodyContract {
            required: true,
            content: BTreeMap::from([(
                "application/json".to_owned(),
                MediaTypeContract {
                    schema: Some(json!({
                        "type": "object",
                        "required": [
                            "customerId", "lower", "upper", "amount", "quantity", "unitAmount",
                            "total", "lineCount", "lineAmounts", "lineMinimum", "lineMaximum",
                            "lineIds", "enabled", "supported"
                        ],
                        "properties": {
                            "customerId": {"type": "string"},
                            "startsAt": {"type": "integer"},
                            "endsAt": {"type": "integer"},
                            "active": {"type": "boolean"},
                            "publishedAt": {"type": ["string", "null"], "format": "date-time"},
                            "archivedAt": {"type": ["string", "null"], "format": "date-time"},
                            "checkoutEnabled": {"type": "boolean"},
                            "lower": {"type": "integer"},
                            "upper": {"type": "integer"},
                            "amount": {"type": "integer"},
                            "quantity": {"type": "integer"},
                            "unitAmount": {"type": "integer"},
                            "total": {"type": "integer"},
                            "lineCount": {"type": "integer"},
                            "lineAmounts": {
                                "type": "array",
                                "items": {"type": "integer"}
                            },
                            "lineMinimum": {"type": "integer"},
                            "lineMaximum": {"type": "integer"},
                            "lineIds": {
                                "type": "array",
                                "items": {"type": "string", "enum": ["a", "b", "c"]}
                            },
                            "enabled": {
                                "type": "array",
                                "items": {"type": "string", "enum": ["card", "bank"]}
                            },
                            "supported": {
                                "type": "array",
                                "items": {"type": "string", "enum": ["card", "bank"]}
                            }
                        },
                        "x-simile-constraints": [
                            {"ordered": {"paths": ["/startsAt", "/endsAt"]}},
                            {"bounded": {
                                "path": "/amount",
                                "minimum": {"path": "/lower"},
                                "maximum": {"path": "/upper"}
                            }},
                            {"equation": {
                                "target": "/total",
                                "value": {"add": [
                                    {"multiply": [
                                        {"path": "/quantity"},
                                        {"path": "/unitAmount"}
                                    ]},
                                    {"literal": 2}
                                ]}
                            }},
                            {"aggregate": {
                                "target": "/lineCount",
                                "operation": "count",
                                "source": "/lineIds/*"
                            }},
                            {"aggregate": {
                                "target": "/lineMinimum",
                                "operation": "min",
                                "source": "/lineAmounts/*"
                            }},
                            {"aggregate": {
                                "target": "/lineMaximum",
                                "operation": "max",
                                "source": "/lineAmounts/*"
                            }},
                            {"distinct": {"source": "/lineIds/*"}},
                            {"set": {
                                "left": "/enabled",
                                "relation": "subset-of",
                                "right": "/supported"
                            }},
                            {
                                "implies": {
                                    "when": {"path": "/active", "equals": false},
                                    "then": {"all": [
                                        {"path": "/publishedAt", "state": "null-or-absent"},
                                        {"path": "/archivedAt", "state": "non-null"},
                                        {"path": "/checkoutEnabled", "equals": false}
                                    ]}
                                }
                            }
                        ]
                    })),
                    example: None,
                    examples: BTreeMap::new(),
                    request_relation: None,
                },
            )]),
        }),
        responses: vec![ResponseContract {
            status: ResponseStatus::Exact(201),
            definition: json!({
                "description": "Created",
                "content": {
                    "application/json": {
                        "schema": {
                            "type": "object",
                            "required": ["customerId", "id"],
                            "properties": {
                                "customerId": {"type": "string"},
                                "id": {"type": "string"}
                            }
                        }
                    }
                }
            }),
            headers: Vec::new(),
            body: Some(ResponseBodyContract {
                content: BTreeMap::from([(
                    "application/json".to_owned(),
                    MediaTypeContract {
                        schema: Some(json!({
                            "type": "object",
                            "required": ["customerId", "id"],
                            "properties": {
                                "customerId": {"type": "string"},
                                "id": {"type": "string"}
                            }
                        })),
                        example: None,
                        examples: BTreeMap::new(),
                        request_relation: None,
                    },
                )]),
            }),
        }],
        security: Vec::new(),
    };
    let resource = Resource {
        kind: ResourceKind::Db,
        name: "orders".to_owned(),
    };
    let resource_ref = resource.reference();
    let asynchronous = AsyncOperation {
        name: "recordOrder".to_owned(),
        uses: vec![resource_ref.clone()],
        actions: Vec::new(),
    };
    let compiled = CompiledOperation {
        contract: operation.clone(),
        origin: BehaviorOrigin::Hcl,
        behavior: Some(Behavior {
            operation_id: "createOrder".to_owned(),
            response_status: Some(201),
            error_sets: Vec::new(),
            outcomes: Vec::new(),
        }),
        outcomes: Vec::new(),
    };
    CompiledService {
        descriptions: {
            let mut descriptions = DescriptionCatalog::default();
            descriptions.insert(DescriptionEntry {
                kind: DescriptionKind::Behavior,
                parent: None,
                name: "createOrder".to_owned(),
                text: "Create an order for one customer.".to_owned(),
            });
            descriptions
        },
        contract: ApiContract {
            openapi: OpenApiVersion {
                exact: "3.2.1".to_owned(),
                feature: OpenApiFeature::V3_2,
            },
            info: ServiceInfo {
                title: "Orders".to_owned(),
                version: "2026-11-15".to_owned(),
                description: None,
            },
            operations: BTreeMap::from([(key.clone(), operation)]),
            security_schemes: BTreeMap::new(),
            webhooks: BTreeMap::new(),
            schemas: BTreeMap::new(),
        },
        heuristic_policy: simile_heuristics::Policy::Standard,
        operations: BTreeMap::from([(key, compiled)]),
        protocol_errors: BTreeMap::new(),
        scenarios: BTreeMap::new(),
        faults: BTreeMap::new(),
        fault_profiles: BTreeMap::new(),
        resources: BTreeMap::from([(resource_ref, resource)]),
        async_operations: BTreeMap::from([(asynchronous.name.clone(), asynchronous)]),
        effects: BTreeMap::new(),
        webhooks: BTreeMap::new(),
        webhook_policy: simile_spec::WebhookPolicy::default(),
        principals: BTreeMap::new(),
        profiles: BTreeMap::new(),
        principal_setups: BTreeMap::new(),
    }
}
