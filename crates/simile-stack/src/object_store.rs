use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{CommandContext, ErrorCode, LogicalTime, ResourceError, Result};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "etag")]
pub enum ObjectWriteCondition {
    #[default]
    Any,
    Absent,
    Etag(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectMetadata {
    pub key: String,
    pub etag: String,
    pub size: usize,
    pub content_type: String,
    pub updated_at: LogicalTime,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredObject {
    pub metadata: ObjectMetadata,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "action"
)]
pub enum ObjectCommand {
    Put {
        key: String,
        bytes: Vec<u8>,
        content_type: String,
        #[serde(default)]
        metadata: BTreeMap<String, String>,
        #[serde(default)]
        condition: ObjectWriteCondition,
    },
    Get {
        key: String,
    },
    Head {
        key: String,
    },
    Delete {
        key: String,
        #[serde(default)]
        condition: ObjectWriteCondition,
    },
    List {
        #[serde(default)]
        prefix: String,
        #[serde(default = "default_list_limit")]
        limit: usize,
    },
}

const fn default_list_limit() -> usize {
    100
}

impl ObjectCommand {
    #[must_use]
    pub const fn action(&self) -> &'static str {
        match self {
            Self::Put { .. } => "put",
            Self::Get { .. } => "get",
            Self::Head { .. } => "head",
            Self::Delete { .. } => "delete",
            Self::List { .. } => "list",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "result", content = "value")]
pub enum ObjectResult {
    Object(Option<StoredObject>),
    Metadata(Option<ObjectMetadata>),
    Stored(ObjectMetadata),
    Deleted(bool),
    Objects(Vec<ObjectMetadata>),
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ObjectEngine {
    pub(crate) resources: BTreeMap<String, BTreeMap<String, StoredObject>>,
}

impl ObjectEngine {
    pub(crate) fn execute(
        &mut self,
        resource: &str,
        context: &CommandContext,
        command: ObjectCommand,
    ) -> Result<ObjectResult> {
        match command {
            ObjectCommand::Put {
                key,
                bytes,
                content_type,
                metadata,
                condition,
            } => {
                if key.is_empty() {
                    return Err(ResourceError::new(
                        ErrorCode::InvalidValue,
                        "an object key cannot be empty",
                    ));
                }
                let current = self.object(resource, &key);
                check_condition(current, &condition)?;
                let object_metadata = ObjectMetadata {
                    key: key.clone(),
                    etag: etag(&bytes),
                    size: bytes.len(),
                    content_type,
                    updated_at: context.at,
                    metadata,
                };
                self.resources
                    .entry(resource.to_owned())
                    .or_default()
                    .insert(
                        key,
                        StoredObject {
                            metadata: object_metadata.clone(),
                            bytes,
                        },
                    );
                Ok(ObjectResult::Stored(object_metadata))
            }
            ObjectCommand::Get { key } => {
                Ok(ObjectResult::Object(self.object(resource, &key).cloned()))
            }
            ObjectCommand::Head { key } => Ok(ObjectResult::Metadata(
                self.object(resource, &key)
                    .map(|object| object.metadata.clone()),
            )),
            ObjectCommand::Delete { key, condition } => {
                let current = self.object(resource, &key);
                check_condition(current, &condition)?;
                Ok(ObjectResult::Deleted(
                    self.resources
                        .get_mut(resource)
                        .and_then(|objects| objects.remove(&key))
                        .is_some(),
                ))
            }
            ObjectCommand::List { prefix, limit } => Ok(ObjectResult::Objects(
                self.resources
                    .get(resource)
                    .into_iter()
                    .flat_map(|objects| objects.range(prefix.clone()..))
                    .take_while(|(key, _)| key.starts_with(&prefix))
                    .take(limit)
                    .map(|(_, object)| object.metadata.clone())
                    .collect(),
            )),
        }
    }

    fn object(&self, resource: &str, key: &str) -> Option<&StoredObject> {
        self.resources.get(resource)?.get(key)
    }
}

fn check_condition(current: Option<&StoredObject>, condition: &ObjectWriteCondition) -> Result<()> {
    let satisfied = match condition {
        ObjectWriteCondition::Any => true,
        ObjectWriteCondition::Absent => current.is_none(),
        ObjectWriteCondition::Etag(expected) => {
            current.is_some_and(|object| object.metadata.etag == *expected)
        }
    };
    if satisfied {
        Ok(())
    } else {
        Err(ResourceError::new(
            ErrorCode::Conflict,
            "the object changed before the conditional write could be applied",
        ))
    }
}

fn etag(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        })
}

#[cfg(test)]
mod tests {
    use super::etag;

    #[test]
    fn etags_are_content_addressed() {
        assert_eq!(etag(b"same"), etag(b"same"));
        assert_ne!(etag(b"same"), etag(b"different"));
    }
}
