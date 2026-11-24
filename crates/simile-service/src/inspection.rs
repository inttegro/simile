//! Read-only views over the running service state.

use simile_stack::{JournalEntry, ResourceAddress};

use crate::{Result, Service, lock};

/// Read-only runtime state for local inspection tools.
///
/// This view is produced by the running service, so it never competes with the
/// service for ownership of its durable state file. Journal entries remain
/// payload-free. Resource values are included only when the caller opts in.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStateInspection {
    pub resources: Vec<InspectedResource>,
    pub events: Vec<JournalEntry>,
}

/// One declared Stack resource and, optionally, its current JSON state.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectedResource {
    pub address: ResourceAddress,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
}

impl Service {
    /// Inspect the state currently owned by this running service.
    ///
    /// Offline commands open the durable store under an exclusive lock. Live
    /// tools must use this method instead so there is one state owner and one
    /// source of truth while the service is serving requests.
    pub fn inspect_state(&self, include_values: bool) -> Result<ServiceStateInspection> {
        let stack = lock(&self.stack)?;
        let snapshot = include_values.then(|| stack.snapshot()).transpose()?;
        let resources = stack
            .resources()
            .iter()
            .cloned()
            .map(|address| InspectedResource {
                value: snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.resource_state(&address)),
                address,
            })
            .collect();
        let events = stack.journal().entries().to_vec();
        Ok(ServiceStateInspection { resources, events })
    }
}
