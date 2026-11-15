use std::collections::BTreeMap;

use serde_json::json;
use simile_artifact::{Error, decode, encode};
use simile_spec::{
    ApiContract, AsyncOperation, Behavior, BehaviorOrigin, CompiledOperation, CompiledService,
    HttpMethod, MediaTypeContract, OpenApiFeature, OpenApiVersion, Operation, OperationKey,
    RequestBodyContract, Resource, ResourceKind, ResponseContract, ResponseStatus, ServiceInfo,
};

#[test]
fn encoding_is_deterministic_and_round_trips_metadata() {
    let service = example_service();
    let first = encode(&service).unwrap_or_else(|error| panic!("{error}"));
    let second = encode(&service).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(first, second);

    let artifact = decode(&first).unwrap_or_else(|error| panic!("{error}"));
    let summary = artifact.summary();
    assert_eq!(summary.format_version, 2);
    assert_eq!(summary.service, "Orders");
    assert_eq!(summary.service_version, "2026-11-15");
    assert_eq!(summary.openapi_version, "3.2.1");
    assert_eq!(summary.operations, 1);
    assert_eq!(summary.request_bodies, 1);
    assert!(summary.request_schema_nodes > 0);
    assert_eq!(summary.responses, 1);
    assert_eq!(summary.resources, 1);
    assert_eq!(summary.async_operations, 1);
    assert_eq!(summary.encoded_bytes, first.len());
    assert_eq!(summary.payload_sha256.len(), 64);
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

fn example_service() -> CompiledService {
    let method = HttpMethod::Post;
    let key = OperationKey::new(&method, "/orders");
    let operation = Operation {
        key: key.clone(),
        operation_id: Some("createOrder".to_owned()),
        method,
        path: "/orders".to_owned(),
        summary: Some("Create an order".to_owned()),
        request_body: Some(json!({"required": true, "schema": {"type": "object"}})),
        request: Some(RequestBodyContract {
            required: true,
            content: BTreeMap::from([(
                "application/json".to_owned(),
                MediaTypeContract {
                    schema: Some(json!({
                        "type": "object",
                        "required": ["customerId"],
                        "properties": {"customerId": {"type": "string"}}
                    })),
                },
            )]),
        }),
        responses: vec![ResponseContract {
            status: ResponseStatus::Exact(201),
            definition: json!({"description": "Created"}),
        }],
    };
    let resource = Resource {
        kind: ResourceKind::Db,
        name: "orders".to_owned(),
    };
    let resource_ref = resource.reference();
    let asynchronous = AsyncOperation {
        name: "recordOrder".to_owned(),
        uses: vec![resource_ref.clone()],
    };
    let compiled = CompiledOperation {
        contract: operation.clone(),
        origin: BehaviorOrigin::Hcl,
        behavior: Some(Behavior {
            operation_id: "createOrder".to_owned(),
            response_status: Some(201),
            triggers: vec![asynchronous.name.clone()],
        }),
    };
    CompiledService {
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
            schemas: BTreeMap::new(),
        },
        operations: BTreeMap::from([(key, compiled)]),
        resources: BTreeMap::from([(resource_ref, resource)]),
        async_operations: BTreeMap::from([(asynchronous.name.clone(), asynchronous)]),
    }
}
