use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CommandContext, ErrorCode, ResourceError, Result};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "version")]
pub enum WriteCondition {
    #[default]
    Any,
    Absent,
    Version(u64),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredDocument {
    pub key: String,
    pub value: Value,
    pub version: u64,
    pub updated_at: crate::LogicalTime,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbFilter {
    pub pointer: String,
    pub equals: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "action"
)]
pub enum DbMutation {
    Put {
        collection: String,
        key: String,
        value: Value,
        #[serde(default)]
        condition: WriteCondition,
    },
    Delete {
        collection: String,
        key: String,
        #[serde(default)]
        condition: WriteCondition,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "action"
)]
pub enum DbCommand {
    Get {
        collection: String,
        key: String,
    },
    Put {
        collection: String,
        key: String,
        value: Value,
        #[serde(default)]
        condition: WriteCondition,
    },
    Merge {
        collection: String,
        key: String,
        value: Value,
        #[serde(default)]
        condition: WriteCondition,
    },
    Delete {
        collection: String,
        key: String,
        #[serde(default)]
        condition: WriteCondition,
    },
    Query {
        collection: String,
        #[serde(default)]
        filters: Vec<DbFilter>,
        #[serde(default = "default_query_limit")]
        limit: usize,
    },
    Transact {
        mutations: Vec<DbMutation>,
    },
}

const fn default_query_limit() -> usize {
    100
}

impl DbCommand {
    #[must_use]
    pub const fn action(&self) -> &'static str {
        match self {
            Self::Get { .. } => "get",
            Self::Put { .. } => "put",
            Self::Merge { .. } => "merge",
            Self::Delete { .. } => "delete",
            Self::Query { .. } => "query",
            Self::Transact { .. } => "transact",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "result", content = "value")]
pub enum DbResult {
    Document(Option<StoredDocument>),
    Documents(Vec<StoredDocument>),
    Written(StoredDocument),
    Deleted(bool),
    Transaction(Vec<DbResult>),
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct DbEngine {
    pub(crate) resources: BTreeMap<String, BTreeMap<String, BTreeMap<String, StoredDocument>>>,
}

impl DbEngine {
    pub(crate) fn execute(
        &mut self,
        resource: &str,
        context: &CommandContext,
        command: DbCommand,
    ) -> Result<DbResult> {
        match command {
            DbCommand::Get { collection, key } => Ok(DbResult::Document(
                self.document(resource, &collection, &key).cloned(),
            )),
            DbCommand::Put {
                collection,
                key,
                value,
                condition,
            } => self
                .put(resource, context, &collection, &key, value, &condition)
                .map(DbResult::Written),
            DbCommand::Merge {
                collection,
                key,
                value,
                condition,
            } => self
                .merge(resource, context, &collection, &key, value, &condition)
                .map(DbResult::Written),
            DbCommand::Delete {
                collection,
                key,
                condition,
            } => self
                .delete(resource, &collection, &key, &condition)
                .map(DbResult::Deleted),
            DbCommand::Query {
                collection,
                filters,
                limit,
            } => Ok(DbResult::Documents(self.query(
                resource,
                &collection,
                &filters,
                limit,
            ))),
            DbCommand::Transact { mutations } => {
                let mut candidate = self.clone();
                let mut results = Vec::with_capacity(mutations.len());
                for mutation in mutations {
                    results.push(candidate.mutate(resource, context, mutation)?);
                }
                *self = candidate;
                Ok(DbResult::Transaction(results))
            }
        }
    }

    fn mutate(
        &mut self,
        resource: &str,
        context: &CommandContext,
        mutation: DbMutation,
    ) -> Result<DbResult> {
        match mutation {
            DbMutation::Put {
                collection,
                key,
                value,
                condition,
            } => self
                .put(resource, context, &collection, &key, value, &condition)
                .map(DbResult::Written),
            DbMutation::Delete {
                collection,
                key,
                condition,
            } => self
                .delete(resource, &collection, &key, &condition)
                .map(DbResult::Deleted),
        }
    }

    fn document(&self, resource: &str, collection: &str, key: &str) -> Option<&StoredDocument> {
        self.resources.get(resource)?.get(collection)?.get(key)
    }

    fn check_condition(current: Option<&StoredDocument>, condition: &WriteCondition) -> Result<()> {
        let satisfied = match condition {
            WriteCondition::Any => true,
            WriteCondition::Absent => current.is_none(),
            WriteCondition::Version(expected) => {
                current.is_some_and(|document| document.version == *expected)
            }
        };
        if satisfied {
            Ok(())
        } else {
            Err(ResourceError::new(
                ErrorCode::Conflict,
                "the document changed before the conditional write could be applied",
            ))
        }
    }

    fn put(
        &mut self,
        resource: &str,
        context: &CommandContext,
        collection: &str,
        key: &str,
        value: Value,
        condition: &WriteCondition,
    ) -> Result<StoredDocument> {
        require_name(collection, "collection")?;
        require_name(key, "document key")?;
        let current = self.document(resource, collection, key);
        Self::check_condition(current, condition)?;
        let version = current.map_or(1, |document| document.version.saturating_add(1));
        let document = StoredDocument {
            key: key.to_owned(),
            value,
            version,
            updated_at: context.at,
        };
        self.resources
            .entry(resource.to_owned())
            .or_default()
            .entry(collection.to_owned())
            .or_default()
            .insert(key.to_owned(), document.clone());
        Ok(document)
    }

    fn delete(
        &mut self,
        resource: &str,
        collection: &str,
        key: &str,
        condition: &WriteCondition,
    ) -> Result<bool> {
        let current = self.document(resource, collection, key);
        Self::check_condition(current, condition)?;
        Ok(self
            .resources
            .get_mut(resource)
            .and_then(|collections| collections.get_mut(collection))
            .and_then(|documents| documents.remove(key))
            .is_some())
    }

    fn merge(
        &mut self,
        resource: &str,
        context: &CommandContext,
        collection: &str,
        key: &str,
        value: Value,
        condition: &WriteCondition,
    ) -> Result<StoredDocument> {
        // Why: public update responses may be partial snapshots. A distinct merge command keeps
        // `put` replacement semantics honest while preventing omitted public fields from being
        // erased when a provider explicitly models a patch-like mutation.
        let mut merged = self.document(resource, collection, key).map_or_else(
            || Value::Object(serde_json::Map::new()),
            |document| document.value.clone(),
        );
        merge_values(&mut merged, value);
        self.put(resource, context, collection, key, merged, condition)
    }

    fn query(
        &self,
        resource: &str,
        collection: &str,
        filters: &[DbFilter],
        limit: usize,
    ) -> Vec<StoredDocument> {
        self.resources
            .get(resource)
            .and_then(|collections| collections.get(collection))
            .into_iter()
            .flat_map(|documents| documents.values())
            .filter(|document| {
                filters
                    .iter()
                    .all(|filter| document.value.pointer(&filter.pointer) == Some(&filter.equals))
            })
            .take(limit)
            .cloned()
            .collect()
    }
}

fn merge_values(target: &mut Value, update: Value) {
    match (target, update) {
        (Value::Object(target), Value::Object(update)) => {
            for (name, value) in update {
                if let Some(existing) = target.get_mut(&name) {
                    merge_values(existing, value);
                } else {
                    target.insert(name, value);
                }
            }
        }
        (target, update) => *target = update,
    }
}

fn require_name(value: &str, label: &str) -> Result<()> {
    if value.is_empty() {
        Err(ResourceError::new(
            ErrorCode::InvalidValue,
            format!("{label} cannot be empty"),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{DbCommand, DbEngine, DbMutation, DbResult, WriteCondition};
    use crate::{CommandContext, CommandId, CommandMetadata, ErrorCode, LogicalTime};

    fn context() -> CommandContext {
        CommandContext {
            command_id: CommandId(1),
            at: LogicalTime::from_unix_millis(10),
            metadata: CommandMetadata::default(),
        }
    }

    #[test]
    fn transactions_are_atomic_when_a_condition_fails() {
        let mut engine = DbEngine::default();
        let result = engine.execute(
            "primary",
            &context(),
            DbCommand::Transact {
                mutations: vec![
                    DbMutation::Put {
                        collection: "customers".to_owned(),
                        key: "one".to_owned(),
                        value: json!({"name": "Ama"}),
                        condition: WriteCondition::Any,
                    },
                    DbMutation::Put {
                        collection: "customers".to_owned(),
                        key: "one".to_owned(),
                        value: json!({"name": "Yaw"}),
                        condition: WriteCondition::Absent,
                    },
                ],
            },
        );
        assert_eq!(result.map_err(|error| error.code), Err(ErrorCode::Conflict));
        assert_eq!(
            engine.execute(
                "primary",
                &context(),
                DbCommand::Get {
                    collection: "customers".to_owned(),
                    key: "one".to_owned(),
                }
            ),
            Ok(DbResult::Document(None))
        );
    }

    #[test]
    fn merge_preserves_omitted_fields_and_replaces_supplied_values() {
        let mut engine = DbEngine::default();
        engine
            .execute(
                "primary",
                &context(),
                DbCommand::Put {
                    collection: "products".to_owned(),
                    key: "prod_one".to_owned(),
                    value: json!({
                        "id": "prod_one",
                        "name": "First edition",
                        "active": true,
                        "dimensions": {"width": 10, "height": 20}
                    }),
                    condition: WriteCondition::Any,
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));

        let result = engine
            .execute(
                "primary",
                &context(),
                DbCommand::Merge {
                    collection: "products".to_owned(),
                    key: "prod_one".to_owned(),
                    value: json!({
                        "name": "Second edition",
                        "dimensions": {"width": 12}
                    }),
                    condition: WriteCondition::Any,
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));

        let DbResult::Written(document) = result else {
            panic!("expected written document");
        };
        assert_eq!(
            document.value,
            json!({
                "id": "prod_one",
                "name": "Second edition",
                "active": true,
                "dimensions": {"width": 12, "height": 20}
            })
        );
        assert_eq!(document.version, 2);
    }
}
