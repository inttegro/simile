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
) -> Result<StateResolution> {
    let Some(operation) = service.artifact().operation(operation_key) else {
        return Ok(StateResolution::None);
    };
    if operation.method() != "GET" {
        return Ok(StateResolution::None);
    }
    let Some(database) = sole_database(service.artifact()) else {
        return Ok(StateResolution::None);
    };
    let Some(collection) = collection_name(operation.path()) else {
        return Ok(StateResolution::None);
    };

    if let Some(key) = path_identity(parameters) {
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
        return candidate(operation, entity_candidates(&collection, document.value))
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
    candidate(operation, list_candidates(&collection, values)).map(StateResolution::Found)
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

fn candidate(
    operation: ArtifactOperation<'_>,
    candidates: impl IntoIterator<Item = Value>,
) -> Result<Value> {
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
    candidates
        .into_iter()
        .find(|candidate| selected.validate(candidate).is_empty())
        .ok_or(Error::StateResponseUnavailable)
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
        "api" | "create" | "new" | "update" | "edit" | "replace" | "delete" | "remove"
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
