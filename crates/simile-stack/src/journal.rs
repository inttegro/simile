use serde::{Deserialize, Serialize};

use crate::{CommandId, ErrorCode, LogicalTime, ResourceAddress};

/// Whether a resource command completed or failed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum JournalOutcome {
    Succeeded,
    Failed { code: ErrorCode },
}

/// One causal, payload-free record of resource activity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    pub sequence: u64,
    pub command_id: CommandId,
    pub at: LogicalTime,
    pub resource: ResourceAddress,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    pub outcome: JournalOutcome,
}

/// Filters an append-only stack journal without exposing command payloads.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResourceAddress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    #[serde(default)]
    pub after_sequence: u64,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

const fn default_limit() -> usize {
    100
}

impl Default for EventQuery {
    fn default() -> Self {
        Self {
            resource: None,
            causation_id: None,
            after_sequence: 0,
            limit: default_limit(),
        }
    }
}

/// The local audit history for one running stack.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Journal {
    entries: Vec<JournalEntry>,
}

impl Journal {
    pub(crate) fn push(&mut self, entry: JournalEntry) {
        self.entries.push(entry);
    }

    #[must_use]
    pub fn query(&self, query: &EventQuery) -> Vec<&JournalEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.sequence > query.after_sequence)
            .filter(|entry| {
                query
                    .resource
                    .as_ref()
                    .is_none_or(|resource| resource == &entry.resource)
            })
            .filter(|entry| {
                query
                    .causation_id
                    .as_deref()
                    .is_none_or(|causation| entry.causation_id.as_deref() == Some(causation))
            })
            .take(query.limit)
            .collect()
    }

    #[must_use]
    pub fn entries(&self) -> &[JournalEntry] {
        &self.entries
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}
