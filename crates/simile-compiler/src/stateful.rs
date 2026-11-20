use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use simile_spec::{
    ApiContract, BehaviorSpec, Effect, EffectAction, EffectActionKind, EffectOrigin, EffectValue,
    HttpMethod, Operation, ResourceKind, ResponseStatus,
};

/// Add the ordinary database consequence that can be proven from public data.
///
/// Inference deliberately stops unless all of these facts are unambiguous:
/// the service has exactly one database, the operation creates, replaces, or updates a
/// resource, one successful response entity has an `id`, and no authored
/// effect already owns that operation. Providers use HCL for every other case.
pub(crate) fn infer_default_effects(contract: &ApiContract, behaviors: &mut BehaviorSpec) {
    let Some(database) = sole_database(behaviors) else {
        return;
    };
    let explicitly_owned = behaviors
        .effects
        .values()
        .flat_map(|effect| effect.operations.iter().cloned())
        .collect::<BTreeSet<_>>();

    for operation in contract.operations.values() {
        let Some(operation_id) = operation.operation_id.as_deref() else {
            continue;
        };
        if explicitly_owned.contains(operation_id) || !creates_or_replaces(operation) {
            continue;
        }
        let Some(entity) = response_entity(contract, operation) else {
            continue;
        };
        let Some(collection) = collection_name(operation.path.as_str(), entity.fallback_name)
        else {
            continue;
        };
        let value_pointer = entity.pointer;
        let key_pointer = format!("{value_pointer}/id");
        let arguments = BTreeMap::from([
            (
                "collection".to_owned(),
                EffectValue::Literal {
                    value: json!(collection),
                },
            ),
            (
                "key".to_owned(),
                EffectValue::Response {
                    pointer: key_pointer,
                },
            ),
            (
                "value".to_owned(),
                EffectValue::Response {
                    pointer: value_pointer,
                },
            ),
        ]);
        let name = format!("simile.inferred.{operation_id}.db_write");
        behaviors.effects.insert(
            name.clone(),
            Effect {
                name,
                origin: EffectOrigin::ConventionInferred,
                operations: vec![operation_id.to_owned()],
                actions: vec![EffectAction {
                    resource: database.clone(),
                    kind: EffectActionKind::DbPut,
                    arguments,
                }],
                async_operations: Vec::new(),
                webhooks: Vec::new(),
            },
        );
    }
}

fn sole_database(behaviors: &BehaviorSpec) -> Option<simile_spec::ResourceRef> {
    let mut databases = behaviors
        .resources
        .values()
        .filter(|resource| resource.kind == ResourceKind::Db)
        .map(simile_spec::Resource::reference);
    let database = databases.next()?;
    databases.next().is_none().then_some(database)
}

fn creates_or_replaces(operation: &Operation) -> bool {
    matches!(
        operation.method,
        HttpMethod::Post | HttpMethod::Put | HttpMethod::Patch
    )
}

struct Entity<'a> {
    pointer: String,
    fallback_name: &'a str,
}

fn response_entity<'a>(contract: &'a ApiContract, operation: &'a Operation) -> Option<Entity<'a>> {
    let mut candidates = operation
        .responses
        .iter()
        .filter(|response| matches!(response.status, ResponseStatus::Exact(200..=299)))
        .filter_map(|response| response.body.as_ref())
        .flat_map(|body| body.content.values())
        .filter_map(|media| media.schema.as_ref())
        .filter_map(|schema| entity_in_schema(contract, schema));
    let entity = candidates.next()?;
    candidates.next().is_none().then_some(entity)
}

fn entity_in_schema<'a>(contract: &'a ApiContract, schema: &'a Value) -> Option<Entity<'a>> {
    let (schema, name) = resolve(contract, schema)?;
    if has_id(schema) {
        return Some(Entity {
            pointer: String::new(),
            fallback_name: name.unwrap_or("resource"),
        });
    }
    let properties = schema.get("properties")?.as_object()?;
    let mut candidates = properties.iter().filter_map(|(field, schema)| {
        let (schema, name) = resolve(contract, schema)?;
        has_id(schema).then(|| Entity {
            pointer: format!("/{}", escape_pointer(field)),
            fallback_name: name.unwrap_or(field),
        })
    });
    let entity = candidates.next()?;
    candidates.next().is_none().then_some(entity)
}

fn resolve<'a>(
    contract: &'a ApiContract,
    schema: &'a Value,
) -> Option<(&'a Value, Option<&'a str>)> {
    let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
        return Some((schema, None));
    };
    let name = reference.strip_prefix("#/components/schemas/")?;
    contract
        .schemas
        .get(name)
        .map(|schema| (schema, Some(name)))
}

fn has_id(schema: &Value) -> bool {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|properties| properties.contains_key("id"))
}

fn collection_name(path: &str, fallback: &str) -> Option<String> {
    let candidate = path
        .split('/')
        .filter(|segment| !segment.is_empty() && !segment.starts_with('{'))
        .filter(|segment| !is_route_scaffolding(segment))
        .next_back()
        .unwrap_or(fallback);
    let normalized = candidate
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_owned();
    (!normalized.is_empty()).then_some(normalized)
}

fn is_route_scaffolding(segment: &str) -> bool {
    matches!(
        segment.to_ascii_lowercase().as_str(),
        "api"
            | "create"
            | "new"
            | "update"
            | "edit"
            | "replace"
            | "delete"
            | "remove"
            | "archive"
            | "restore"
            | "finalize"
    ) || segment
        .strip_prefix('v')
        .is_some_and(|version| !version.is_empty() && version.chars().all(|c| c.is_ascii_digit()))
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
