use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CommandContext, ErrorCode, LogicalTime, ResourceError, Result};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BusEvent {
    pub id: String,
    pub topic: String,
    pub sequence: u64,
    pub body: Value,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
    pub published_at: LogicalTime,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "action"
)]
pub enum BusCommand {
    Publish {
        topic: String,
        body: Value,
        #[serde(default)]
        attributes: BTreeMap<String, String>,
    },
    Read {
        topic: String,
        subscription: String,
        #[serde(default = "default_read_limit")]
        limit: usize,
    },
    Commit {
        topic: String,
        subscription: String,
        sequence: u64,
    },
    Checkpoint {
        topic: String,
        subscription: String,
    },
}

const fn default_read_limit() -> usize {
    100
}

impl BusCommand {
    #[must_use]
    pub const fn action(&self) -> &'static str {
        match self {
            Self::Publish { .. } => "publish",
            Self::Read { .. } => "read",
            Self::Commit { .. } => "commit",
            Self::Checkpoint { .. } => "checkpoint",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "result", content = "value")]
pub enum BusResult {
    Published(BusEvent),
    Events(Vec<BusEvent>),
    Committed(u64),
    Checkpoint(u64),
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct BusState {
    topics: BTreeMap<String, Vec<BusEvent>>,
    checkpoints: BTreeMap<String, BTreeMap<String, u64>>,
    next_event: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct BusEngine {
    pub(crate) resources: BTreeMap<String, BusState>,
}

impl BusEngine {
    pub(crate) fn execute(
        &mut self,
        resource: &str,
        context: &CommandContext,
        command: BusCommand,
    ) -> Result<BusResult> {
        let state = self.resources.entry(resource.to_owned()).or_default();
        match command {
            BusCommand::Publish {
                topic,
                body,
                attributes,
            } => {
                require_name(&topic, "topic")?;
                state.next_event = state.next_event.saturating_add(1);
                let sequence = state
                    .topics
                    .get(&topic)
                    .map_or(1, |events| events.len() as u64 + 1);
                let event = BusEvent {
                    id: format!("evt_{:016x}", state.next_event),
                    topic: topic.clone(),
                    sequence,
                    body,
                    attributes,
                    published_at: context.at,
                };
                state.topics.entry(topic).or_default().push(event.clone());
                Ok(BusResult::Published(event))
            }
            BusCommand::Read {
                topic,
                subscription,
                limit,
            } => {
                require_name(&subscription, "subscription")?;
                let checkpoint = checkpoint(state, &topic, &subscription);
                Ok(BusResult::Events(
                    state
                        .topics
                        .get(&topic)
                        .into_iter()
                        .flat_map(|events| events.iter())
                        .filter(|event| event.sequence > checkpoint)
                        .take(limit)
                        .cloned()
                        .collect(),
                ))
            }
            BusCommand::Commit {
                topic,
                subscription,
                sequence,
            } => {
                let maximum = state
                    .topics
                    .get(&topic)
                    .and_then(|events| events.last())
                    .map_or(0, |event| event.sequence);
                let current = checkpoint(state, &topic, &subscription);
                if sequence < current || sequence > maximum {
                    return Err(ResourceError::new(
                        ErrorCode::Conflict,
                        "the event checkpoint is outside the committed topic history",
                    ));
                }
                state
                    .checkpoints
                    .entry(topic)
                    .or_default()
                    .insert(subscription, sequence);
                Ok(BusResult::Committed(sequence))
            }
            BusCommand::Checkpoint {
                topic,
                subscription,
            } => Ok(BusResult::Checkpoint(checkpoint(
                state,
                &topic,
                &subscription,
            ))),
        }
    }
}

fn checkpoint(state: &BusState, topic: &str, subscription: &str) -> u64 {
    state
        .checkpoints
        .get(topic)
        .and_then(|subscriptions| subscriptions.get(subscription))
        .copied()
        .unwrap_or_default()
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
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::{BusCommand, BusEngine, BusResult};
    use crate::{CommandContext, CommandId, CommandMetadata, LogicalTime};

    fn context() -> CommandContext {
        CommandContext {
            command_id: CommandId(1),
            at: LogicalTime::UNIX_EPOCH,
            metadata: CommandMetadata::default(),
        }
    }

    #[test]
    fn subscriptions_advance_independently() {
        let mut engine = BusEngine::default();
        let published = engine.execute(
            "domain",
            &context(),
            BusCommand::Publish {
                topic: "orders".to_owned(),
                body: json!({"id": "one"}),
                attributes: BTreeMap::new(),
            },
        );
        assert!(matches!(published, Ok(BusResult::Published(_))));
        let committed = engine.execute(
            "domain",
            &context(),
            BusCommand::Commit {
                topic: "orders".to_owned(),
                subscription: "billing".to_owned(),
                sequence: 1,
            },
        );
        assert_eq!(committed, Ok(BusResult::Committed(1)));
        let other = engine.execute(
            "domain",
            &context(),
            BusCommand::Read {
                topic: "orders".to_owned(),
                subscription: "mail".to_owned(),
                limit: 10,
            },
        );
        assert!(matches!(other, Ok(BusResult::Events(events)) if events.len() == 1));
    }
}
