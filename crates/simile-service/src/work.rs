use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use simile_runtime::{OperationExecution, PlannedAction, PlannedWebhook};
use simile_stack::{QueueMessage, StackCommand};

use crate::{Error, Result};

pub(crate) const OUTBOX: &str = "__simile_work";
pub(crate) const DEAD_LETTERS: &str = "__simile_dead_letters";
pub(crate) const WORK_STATE: &str = "__simile_work_state";
pub(crate) const DEFAULT_MAX_ATTEMPTS: u32 = 5;
pub(crate) const DEFAULT_VISIBILITY_TIMEOUT_MS: u64 = 30_000;

/// A durable consequence that can be leased and retried independently of the
/// request that caused it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum DeferredWork {
    Async {
        name: String,
        operation: String,
        commands: Vec<DeferredCommand>,
        #[serde(default)]
        failures: u32,
        max_attempts: u32,
    },
    Webhook {
        name: String,
        operation: String,
        correlation_id: String,
        method: String,
        content_type: Option<String>,
        target: String,
        body: Value,
        after: Vec<String>,
        #[serde(default)]
        failures: u32,
        max_attempts: u32,
    },
}

impl DeferredWork {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Async { name, .. } | Self::Webhook { name, .. } => name,
        }
    }

    #[must_use]
    pub fn operation(&self) -> &str {
        match self {
            Self::Async { operation, .. } | Self::Webhook { operation, .. } => operation,
        }
    }

    #[must_use]
    pub const fn max_attempts(&self) -> u32 {
        match self {
            Self::Async { max_attempts, .. } | Self::Webhook { max_attempts, .. } => *max_attempts,
        }
    }

    #[must_use]
    pub const fn failures(&self) -> u32 {
        match self {
            Self::Async { failures, .. } | Self::Webhook { failures, .. } => *failures,
        }
    }

    #[must_use]
    pub fn after_failure(&self) -> Self {
        let mut work = self.clone();
        match &mut work {
            Self::Async { failures, .. } | Self::Webhook { failures, .. } => {
                *failures = failures.saturating_add(1);
            }
        }
        work
    }

    #[must_use]
    pub fn with_fresh_retry_budget(&self) -> Self {
        let mut work = self.clone();
        match &mut work {
            Self::Async { failures, .. } | Self::Webhook { failures, .. } => *failures = 0,
        }
        work
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeferredCommand {
    pub resource: String,
    pub command: StackCommand,
}

/// One leased work item. Dropping the lease leaves the item in the outbox; it
/// becomes available again after the visibility timeout.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkLease {
    pub(crate) message_id: String,
    pub(crate) receipt_handle: String,
    pub(crate) work: DeferredWork,
}

/// One exhausted work item leased from the dead-letter queue for inspection
/// or explicit redelivery.
#[derive(Clone, Debug, PartialEq)]
pub struct DeadLetter {
    pub(crate) lease: WorkLease,
}

impl DeadLetter {
    #[must_use]
    pub fn id(&self) -> &str {
        self.lease.id()
    }

    #[must_use]
    pub const fn attempts(&self) -> u32 {
        self.lease.attempt()
    }

    #[must_use]
    pub const fn work(&self) -> &DeferredWork {
        self.lease.work()
    }
}

impl WorkLease {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.message_id
    }

    #[must_use]
    pub const fn attempt(&self) -> u32 {
        self.work.failures().saturating_add(1)
    }

    #[must_use]
    pub const fn work(&self) -> &DeferredWork {
        &self.work
    }
}

pub(crate) struct WorkEnvelope {
    pub work: DeferredWork,
    pub delay_milliseconds: u64,
}

pub(crate) fn planned(
    execution: &OperationExecution,
    correlation_id: &str,
) -> Result<Vec<WorkEnvelope>> {
    let mut work = execution
        .effects()
        .asynchronous()
        .iter()
        .map(|operation| {
            Ok(WorkEnvelope {
                work: DeferredWork::Async {
                    name: operation.name().to_owned(),
                    operation: execution.operation().to_owned(),
                    commands: operation
                        .actions()
                        .iter()
                        .map(deferred_command)
                        .collect::<Result<Vec<_>>>()?,
                    failures: 0,
                    max_attempts: DEFAULT_MAX_ATTEMPTS,
                },
                delay_milliseconds: 0,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    work.extend(
        execution
            .effects()
            .webhooks()
            .iter()
            .filter_map(|webhook| webhook.target().map(|_| webhook))
            .map(|webhook| webhook_work(execution.operation(), correlation_id, webhook))
            .collect::<Vec<_>>(),
    );
    Ok(work)
}

fn deferred_command(action: &PlannedAction) -> Result<DeferredCommand> {
    Ok(DeferredCommand {
        resource: action.resource().as_str().to_owned(),
        command: StackCommand::from_effect(action.kind(), action.arguments())?,
    })
}

fn webhook_work(operation: &str, correlation_id: &str, webhook: &PlannedWebhook) -> WorkEnvelope {
    WorkEnvelope {
        work: DeferredWork::Webhook {
            name: webhook.name().to_owned(),
            operation: operation.to_owned(),
            correlation_id: correlation_id.to_owned(),
            method: webhook.method().to_owned(),
            content_type: webhook.content_type().map(str::to_owned),
            target: webhook.target().unwrap_or_default().to_owned(),
            body: webhook.body().clone(),
            after: webhook.after().to_vec(),
            failures: 0,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
        },
        delay_milliseconds: webhook.delay_milliseconds(),
    }
}

pub(crate) fn decode_lease(message: QueueMessage) -> Result<WorkLease> {
    let receipt_handle = message
        .receipt_handle
        .ok_or_else(|| Error::InvalidWork("leased queue message has no receipt".to_owned()))?;
    let work = serde_json::from_value(message.body)
        .map_err(|error| Error::InvalidWork(error.to_string()))?;
    Ok(WorkLease {
        message_id: message.id,
        receipt_handle,
        work,
    })
}

pub(crate) fn decode_dead_letter(message: QueueMessage) -> Result<DeadLetter> {
    decode_lease(message).map(|lease| DeadLetter { lease })
}

pub(crate) fn encode(work: &DeferredWork) -> Result<Value> {
    serde_json::to_value(work).map_err(|error| Error::InvalidWork(error.to_string()))
}

pub(crate) fn attributes(work: &DeferredWork) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("name".to_owned(), work.name().to_owned()),
        ("operation".to_owned(), work.operation().to_owned()),
    ])
}
