use serde_json::{Map, Value, json};
use simile_artifact::{Artifact, ArtifactOperation};
use simile_runtime::ValidatedParameters;
use simile_spec::{ParameterLocation, ResourceKind, ResponseStatus};
use simile_stack::{DbCommand, DbResult, ResourceAddress, StackCommand, StackResult};

use crate::{Error, Result, Service};

pub(crate) enum StateResolution {
    None,
    Found(Value),
    Missing,
}

pub(crate) fn resolve(
    service: &Service,
    operation_key: &str,
    parameters: Option<&ValidatedParameters>,
    request_body: Option<&Value>,
) -> Result<StateResolution> {
    let Some(operation) = service.artifact().operation(operation_key) else {
        return Ok(StateResolution::None);
    };
    let Some(database) = sole_database(service.artifact()) else {
        return Ok(StateResolution::None);
    };
    let Some(collection) = collection_name(operation.path()) else {
        return Ok(StateResolution::None);
    };
    let Some(read) = read_kind(&operation, parameters, request_body, &collection) else {
        return Ok(StateResolution::None);
    };

    if let ReadKind::Entity(key) = read {
        let receipt = service.read_stack(
            database,
            StackCommand::Db(DbCommand::Get {
                collection: collection.clone(),
                key,
            }),
        )?;
        let document = match receipt.result {
            StackResult::Db(result) => match *result {
                DbResult::Document(document) => document,
                _ => return Err(Error::InvalidStateRead("db.get")),
            },
            _ => return Err(Error::InvalidStateRead("db.get")),
        };
        let Some(document) = document else {
            return Ok(StateResolution::Missing);
        };
        return entity_candidate(operation, &collection, document.value, request_body)
            .map(StateResolution::Found);
    }

    let receipt = service.read_stack(
        database,
        StackCommand::Db(DbCommand::Query {
            collection: collection.clone(),
            filters: Vec::new(),
            limit: 100,
        }),
    )?;
    let documents = match receipt.result {
        StackResult::Db(result) => match *result {
            DbResult::Documents(documents) => documents,
            _ => return Err(Error::InvalidStateRead("db.query")),
        },
        _ => return Err(Error::InvalidStateRead("db.query")),
    };
    let values = Value::Array(
        documents
            .into_iter()
            .map(|document| document.value)
            .collect(),
    );
    match collection_candidate(operation, &collection, values, request_body) {
        Ok(value) => Ok(StateResolution::Found(value)),
        // Why: the terminal path segment alone cannot prove that a GET action is a collection
        // read. Falling through preserves its authored response instead of failing the request.
        Err(Error::StateResponseUnavailable) => Ok(StateResolution::None),
        Err(error) => Err(error),
    }
}

pub(crate) fn delete_command(
    artifact: &Artifact,
    operation_key: &str,
    parameters: Option<&ValidatedParameters>,
) -> Option<(ResourceAddress, StackCommand)> {
    let operation = artifact.operation(operation_key)?;
    if operation.method() != "DELETE" {
        return None;
    }
    let database = sole_database(artifact)?;
    let collection = collection_name(operation.path())?;
    let key = path_identity(parameters)?;
    Some((
        database,
        StackCommand::Db(DbCommand::Delete {
            collection,
            key,
            condition: simile_stack::WriteCondition::Any,
        }),
    ))
}

pub(crate) fn declares_not_found(operation: ArtifactOperation<'_>) -> bool {
    operation
        .responses()
        .any(|response| response.status().matches(404))
}

fn entity_candidate(
    operation: ArtifactOperation<'_>,
    collection: &str,
    entity: Value,
    request_body: Option<&Value>,
) -> Result<Value> {
    let selected = successful_json_response(operation)?;
    let mut candidates = entity_candidates(collection, entity.clone());
    for baseline in response_baselines(&selected, request_body) {
        candidates.extend(replace_entity_candidates(baseline, &entity));
    }
    candidates
        .into_iter()
        .find(|candidate| selected.validate(candidate).is_empty())
        .ok_or(Error::StateResponseUnavailable)
}

fn collection_candidate(
    operation: ArtifactOperation<'_>,
    collection: &str,
    values: Value,
    request_body: Option<&Value>,
) -> Result<Value> {
    let selected = successful_json_response(operation)?;
    let mut candidates = list_candidates(collection, values.clone());
    for baseline in response_baselines(&selected, request_body) {
        candidates.extend(replace_collection_candidates(baseline, &values));
    }
    candidates
        .into_iter()
        .find(|candidate| selected.validate(candidate).is_empty())
        .ok_or(Error::StateResponseUnavailable)
}

fn successful_json_response(
    operation: ArtifactOperation<'_>,
) -> Result<simile_schema::BoundMediaType<'_>> {
    let response = operation
        .responses()
        .filter(|response| match response.status() {
            ResponseStatus::Exact(status) => (200..300).contains(status),
            ResponseStatus::Range(range) => *range == 2,
            ResponseStatus::Default => false,
        })
        .find_map(|response| response.body())
        .ok_or(Error::StateResponseUnavailable)?;
    let media_type = response
        .media_types()
        .find(|media_type| *media_type == "application/json" || media_type.ends_with("+json"))
        .ok_or(Error::StateResponseUnavailable)?;
    let selected = response
        .select(media_type)
        .ok_or(Error::StateResponseUnavailable)?;
    Ok(selected)
}

fn response_baselines(
    selected: &simile_schema::BoundMediaType<'_>,
    request_body: Option<&Value>,
) -> Vec<Value> {
    let mut baselines = Vec::new();
    let ordinary = request_body.map_or_else(
        || selected.synthesize(),
        |request| selected.synthesize_from(request),
    );
    if let Ok(value) = ordinary {
        baselines.push(value);
    }
    let representative = request_body.map_or_else(
        || selected.synthesize_representative(),
        |request| selected.synthesize_representative_from(request),
    );
    if let Ok(value) = representative {
        if !baselines.contains(&value) {
            baselines.push(value);
        }
    }
    baselines
}

fn entity_candidates(collection: &str, entity: Value) -> Vec<Value> {
    let singular = singular(collection);
    let mut candidates = vec![entity.clone()];
    for field in [singular.as_str(), collection] {
        let mut wrapper = Map::new();
        wrapper.insert(field.to_owned(), entity.clone());
        candidates.push(Value::Object(wrapper));
    }
    candidates
}

fn list_candidates(collection: &str, values: Value) -> Vec<Value> {
    let mut collection_wrapper = Map::new();
    collection_wrapper.insert(collection.to_owned(), values.clone());
    vec![
        values.clone(),
        json!({"data": values.clone()}),
        Value::Object(collection_wrapper),
    ]
}

fn replace_entity_candidates(baseline: Value, entity: &Value) -> Vec<Value> {
    let mut paths = Vec::new();
    collect_entity_paths(&baseline, &mut Vec::new(), &mut paths);
    paths
        .into_iter()
        .filter_map(|path| {
            let mut candidate = baseline.clone();
            replace_at_path(&mut candidate, &path, entity.clone()).then_some(candidate)
        })
        .collect()
}

fn collect_entity_paths(value: &Value, path: &mut Vec<String>, paths: &mut Vec<Vec<String>>) {
    let Value::Object(object) = value else {
        return;
    };
    if object.contains_key("id") {
        paths.push(path.clone());
    }
    for (name, child) in object {
        path.push(name.clone());
        collect_entity_paths(child, path, paths);
        path.pop();
    }
}

fn replace_collection_candidates(baseline: Value, values: &Value) -> Vec<Value> {
    let mut paths = Vec::new();
    collect_array_paths(&baseline, &mut Vec::new(), &mut paths);
    paths
        .into_iter()
        .filter_map(|path| {
            let mut candidate = baseline.clone();
            if !replace_at_path(&mut candidate, &path, values.clone()) {
                return None;
            }
            repair_page_metadata(&mut candidate, values.as_array().map_or(0, Vec::len));
            Some(candidate)
        })
        .collect()
}

fn collect_array_paths(value: &Value, path: &mut Vec<String>, paths: &mut Vec<Vec<String>>) {
    match value {
        Value::Array(_) => paths.push(path.clone()),
        Value::Object(object) => {
            for (name, child) in object {
                path.push(name.clone());
                collect_array_paths(child, path, paths);
                path.pop();
            }
        }
        _ => {}
    }
}

fn replace_at_path(value: &mut Value, path: &[String], replacement: Value) -> bool {
    let mut current = value;
    for segment in path {
        let Value::Object(object) = current else {
            return false;
        };
        let Some(next) = object.get_mut(segment) else {
            return false;
        };
        current = next;
    }
    *current = replacement;
    true
}

fn repair_page_metadata(value: &mut Value, length: usize) {
    match value {
        Value::Object(object) => {
            let count = u64::try_from(length).unwrap_or(u64::MAX);
            for field in ["size", "count", "total"] {
                if object.contains_key(field) {
                    object.insert(field.to_owned(), Value::from(count));
                }
            }
            if object.contains_key("has_more") {
                object.insert("has_more".to_owned(), Value::Bool(false));
            }
            for child in object.values_mut() {
                repair_page_metadata(child, length);
            }
        }
        Value::Array(items) => {
            for item in items {
                repair_page_metadata(item, length);
            }
        }
        _ => {}
    }
}

fn sole_database(artifact: &Artifact) -> Option<ResourceAddress> {
    let mut databases = artifact
        .resources()
        .filter(|resource| resource.kind() == ResourceKind::Db)
        .filter_map(|resource| ResourceAddress::new(resource.kind(), resource.name()).ok());
    let database = databases.next()?;
    databases.next().is_none().then_some(database)
}

fn path_identity(parameters: Option<&ValidatedParameters>) -> Option<String> {
    let mut values = parameters?.at(ParameterLocation::Path);
    let (_, value) = values.next()?;
    if values.next().is_some() {
        return None;
    }
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

enum ReadKind {
    Entity(String),
    Collection,
}

fn read_kind(
    operation: &ArtifactOperation<'_>,
    parameters: Option<&ValidatedParameters>,
    request_body: Option<&Value>,
    collection: &str,
) -> Option<ReadKind> {
    match operation.method() {
        "GET" => {
            if let Some(identity) = path_identity(parameters) {
                return Some(ReadKind::Entity(identity));
            }
            // Why: only a route whose terminal semantic segment is the collection is a possible
            // list. Response validation below is the final proof; incompatible actions fall
            // through to their declared outcome.
            (action(operation.path()) == Some(collection)).then_some(ReadKind::Collection)
        }
        "POST" if action(operation.path()) == Some("lookup") => {
            request_identity(request_body?, collection).map(ReadKind::Entity)
        }
        "POST" if action(operation.path()) == Some("page") => Some(ReadKind::Collection),
        _ => None,
    }
}

fn request_identity(request: &Value, collection: &str) -> Option<String> {
    let object = request.as_object()?;
    let preferred = format!("{}_id", singular(collection));
    for name in [preferred.as_str(), "id"] {
        if let Some(value) = object.get(name).and_then(scalar_key) {
            return Some(value);
        }
    }
    let mut identities = object
        .iter()
        .filter(|(name, _)| name.ends_with("_id"))
        .filter_map(|(_, value)| scalar_key(value));
    let identity = identities.next()?;
    identities.next().is_none().then_some(identity)
}

fn scalar_key(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn action(path: &str) -> Option<&str> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .next_back()
}

fn collection_name(path: &str) -> Option<String> {
    let segment = path
        .split('/')
        .filter(|segment| !segment.is_empty() && !segment.starts_with('{'))
        .filter(|segment| !is_scaffolding(segment))
        .next_back()?;
    let normalized = segment
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

fn is_scaffolding(segment: &str) -> bool {
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
            | "lookup"
            | "page"
    ) || segment
        .strip_prefix('v')
        .is_some_and(|version| !version.is_empty() && version.chars().all(|c| c.is_ascii_digit()))
}

fn singular(collection: &str) -> String {
    collection
        .strip_suffix("ies")
        .map(|stem| format!("{stem}y"))
        .or_else(|| collection.strip_suffix('s').map(str::to_owned))
        .unwrap_or_else(|| collection.to_owned())
}
