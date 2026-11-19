use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CommandContext, ErrorCode, LogicalTime, ResourceError, Result};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    pub value: Value,
    pub version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<LogicalTime>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "action"
)]
pub enum CacheCommand {
    Get {
        key: String,
    },
    Set {
        key: String,
        value: Value,
        #[serde(default)]
        ttl_ms: Option<u64>,
    },
    Delete {
        key: String,
    },
    Increment {
        key: String,
        #[serde(default = "default_increment")]
        by: i64,
        #[serde(default)]
        ttl_ms: Option<u64>,
    },
    CompareAndSet {
        key: String,
        expected_version: u64,
        value: Value,
        #[serde(default)]
        ttl_ms: Option<u64>,
    },
}

const fn default_increment() -> i64 {
    1
}

impl CacheCommand {
    #[must_use]
    pub const fn action(&self) -> &'static str {
        match self {
            Self::Get { .. } => "get",
            Self::Set { .. } => "set",
            Self::Delete { .. } => "delete",
            Self::Increment { .. } => "increment",
            Self::CompareAndSet { .. } => "compare_and_set",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "result", content = "value")]
pub enum CacheResult {
    Entry(Option<CacheEntry>),
    Stored(CacheEntry),
    Deleted(bool),
    Integer(i64),
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct CacheEngine {
    pub(crate) resources: BTreeMap<String, BTreeMap<String, CacheEntry>>,
}

impl CacheEngine {
    pub(crate) fn execute(
        &mut self,
        resource: &str,
        context: &CommandContext,
        command: CacheCommand,
    ) -> Result<CacheResult> {
        self.purge_resource(resource, context.at);
        match command {
            CacheCommand::Get { key } => {
                Ok(CacheResult::Entry(self.entry(resource, &key).cloned()))
            }
            CacheCommand::Set { key, value, ttl_ms } => self
                .store(resource, context.at, key, value, ttl_ms, None)
                .map(CacheResult::Stored),
            CacheCommand::Delete { key } => Ok(CacheResult::Deleted(
                self.resources
                    .get_mut(resource)
                    .and_then(|entries| entries.remove(&key))
                    .is_some(),
            )),
            CacheCommand::Increment { key, by, ttl_ms } => {
                let value = self.entry(resource, &key).map_or(Ok(0), |entry| {
                    entry.value.as_i64().ok_or_else(|| {
                        ResourceError::new(
                            ErrorCode::InvalidValue,
                            "the cached value is not an integer",
                        )
                    })
                })?;
                let value = value.checked_add(by).ok_or_else(|| {
                    ResourceError::new(ErrorCode::InvalidValue, "the integer increment overflowed")
                })?;
                self.store(resource, context.at, key, Value::from(value), ttl_ms, None)?;
                Ok(CacheResult::Integer(value))
            }
            CacheCommand::CompareAndSet {
                key,
                expected_version,
                value,
                ttl_ms,
            } => self
                .store(
                    resource,
                    context.at,
                    key,
                    value,
                    ttl_ms,
                    Some(expected_version),
                )
                .map(CacheResult::Stored),
        }
    }

    pub(crate) fn purge_expired(&mut self, now: LogicalTime) -> usize {
        let before = self.resources.values().map(BTreeMap::len).sum::<usize>();
        for entries in self.resources.values_mut() {
            entries.retain(|_, entry| !is_expired(entry, now));
        }
        before - self.resources.values().map(BTreeMap::len).sum::<usize>()
    }

    fn purge_resource(&mut self, resource: &str, now: LogicalTime) {
        if let Some(entries) = self.resources.get_mut(resource) {
            entries.retain(|_, entry| !is_expired(entry, now));
        }
    }

    fn entry(&self, resource: &str, key: &str) -> Option<&CacheEntry> {
        self.resources.get(resource)?.get(key)
    }

    fn store(
        &mut self,
        resource: &str,
        now: LogicalTime,
        key: String,
        value: Value,
        ttl_ms: Option<u64>,
        expected_version: Option<u64>,
    ) -> Result<CacheEntry> {
        if key.is_empty() {
            return Err(ResourceError::new(
                ErrorCode::InvalidValue,
                "a cache key cannot be empty",
            ));
        }
        let current = self.entry(resource, &key);
        if expected_version.is_some() && current.map(|entry| entry.version) != expected_version {
            return Err(ResourceError::new(
                ErrorCode::Conflict,
                "the cached value changed before compare-and-set",
            ));
        }
        let entry = CacheEntry {
            value,
            version: current.map_or(1, |entry| entry.version.saturating_add(1)),
            expires_at: ttl_ms
                .map(|ttl| now.checked_add(ttl))
                .transpose()
                .map_err(|error| ResourceError::new(ErrorCode::InvalidValue, error.to_string()))?,
        };
        self.resources
            .entry(resource.to_owned())
            .or_default()
            .insert(key, entry.clone());
        Ok(entry)
    }
}

fn is_expired(entry: &CacheEntry, now: LogicalTime) -> bool {
    entry.expires_at.is_some_and(|expires_at| expires_at <= now)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CacheCommand, CacheEngine, CacheResult};
    use crate::{CommandContext, CommandId, CommandMetadata, LogicalTime};

    fn context(at: i64) -> CommandContext {
        CommandContext {
            command_id: CommandId(1),
            at: LogicalTime::from_unix_millis(at),
            metadata: CommandMetadata::default(),
        }
    }

    #[test]
    fn entries_expire_on_logical_time() {
        let mut engine = CacheEngine::default();
        let stored = engine.execute(
            "session",
            &context(10),
            CacheCommand::Set {
                key: "token".to_owned(),
                value: json!("value"),
                ttl_ms: Some(5),
            },
        );
        assert!(matches!(stored, Ok(CacheResult::Stored(_))));
        assert_eq!(
            engine.execute(
                "session",
                &context(15),
                CacheCommand::Get {
                    key: "token".to_owned()
                }
            ),
            Ok(CacheResult::Entry(None))
        );
    }
}
