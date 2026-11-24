//! Deferred consequence leasing, dependency supervision, retries, and dead letters.

use std::collections::BTreeMap;

use simile_spec::ResourceKind;
use simile_stack::{
    CommandMetadata, DbCommand, JournalOutcome, LogicalTime, QueueCommand, QueueResult,
    ResourceAddress, Stack, StackCommand, StackResult, StackSnapshot, WriteCondition,
};

use crate::work::{DEAD_LETTERS, DEFAULT_VISIBILITY_TIMEOUT_MS, OUTBOX, WORK_STATE};
use crate::{DeadLetter, DeferredWork, WorkLease};
use crate::{Error, Result, Service, lock, work};

/// Whether every hard prerequisite for one deferred item has completed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DependencyReadiness {
    Ready,
    Waiting,
    Blocked,
}

/// Read-only view of durable work owned by the running service.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeferredWorkInspection {
    pub pending: usize,
    pub leased: usize,
    pub dead_letters: usize,
    pub items: Vec<InspectedDeferredWork>,
}

/// One queued consequence, without its private receipt handle.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectedDeferredWork {
    pub id: String,
    pub queue: String,
    pub published_at: LogicalTime,
    pub available_at: LogicalTime,
    pub receive_count: u32,
    pub leased: bool,
    pub work: DeferredWork,
}

#[derive(serde::Deserialize)]
struct StoredWorkInspection {
    id: String,
    body: serde_json::Value,
    published_at: LogicalTime,
    available_at: LogicalTime,
    receive_count: u32,
    receipt_handle: Option<String>,
}

impl Service {
    /// Lease available deferred work. Unacknowledged items become visible again
    /// after `visibility_timeout_milliseconds`.
    pub fn lease_work(
        &self,
        limit: usize,
        visibility_timeout_milliseconds: Option<u64>,
    ) -> Result<Vec<WorkLease>> {
        let mut stack = lock(&self.stack)?;
        let before = stack.snapshot()?;
        let has_available_work = limit > 0
            && inspect_work_queue(&before, OUTBOX, "outbox")?
                .iter()
                .any(|item| item.available_at <= before.now());
        if !has_available_work {
            return Ok(Vec::new());
        }
        let receipt = match stack.execute(
            ResourceAddress::new(ResourceKind::Queue, OUTBOX)?,
            CommandMetadata::default(),
            StackCommand::Queue(QueueCommand::Receive {
                max_messages: limit,
                visibility_timeout_ms: visibility_timeout_milliseconds
                    .unwrap_or(DEFAULT_VISIBILITY_TIMEOUT_MS),
            }),
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                *stack = Stack::from_snapshot(before);
                return Err(error.into());
            }
        };
        self.checkpoint(&mut stack, before)?;
        match receipt.result {
            StackResult::Queue(result) => match *result {
                QueueResult::Messages(messages) => {
                    messages.into_iter().map(work::decode_lease).collect()
                }
                _ => Err(Error::InvalidWork(
                    "outbox receive returned an unexpected result".to_owned(),
                )),
            },
            _ => Err(Error::InvalidWork(
                "outbox receive returned a non-queue result".to_owned(),
            )),
        }
    }

    /// Execute the typed commands carried by one asynchronous work item.
    pub fn execute_work(&self, lease: &WorkLease) -> Result<()> {
        let DeferredWork::Async {
            operation,
            commands,
            ..
        } = lease.work()
        else {
            return Ok(());
        };
        let mut stack = lock(&self.stack)?;
        let before = stack.snapshot()?;
        for command in commands {
            let resource = command.resource.parse::<ResourceAddress>()?;
            if let Err(error) = stack.execute(
                resource,
                CommandMetadata {
                    operation_id: Some(operation.clone()),
                    causation_id: Some(lease.id().to_owned()),
                    ..CommandMetadata::default()
                },
                command.command.clone(),
            ) {
                *stack = Stack::from_snapshot(before);
                return Err(error.into());
            }
        }
        self.checkpoint(&mut stack, before)
    }

    /// Remove successfully completed work from the durable outbox.
    pub fn acknowledge_work(&self, lease: &WorkLease) -> Result<()> {
        self.finish_lease(lease, LeaseDisposition::Acknowledge)
    }

    /// Release failed work with a retry delay, or move it to dead letters once
    /// its declared attempt budget is exhausted.
    pub fn retry_work(&self, lease: &WorkLease, delay_milliseconds: u64) -> Result<bool> {
        let exhausted = lease.attempt() >= lease.work().max_attempts();
        self.finish_lease(
            lease,
            if exhausted {
                LeaseDisposition::DeadLetter
            } else {
                LeaseDisposition::Retry(delay_milliseconds)
            },
        )?;
        Ok(!exhausted)
    }

    /// Return leased work to the outbox without consuming a delivery attempt.
    ///
    /// The dispatcher uses this when a hard causal prerequisite is still in
    /// flight. Queue receive counts remain transport diagnostics; the retry
    /// budget tracks actual execution failures inside the work envelope.
    pub fn defer_work(&self, lease: &WorkLease, delay_milliseconds: u64) -> Result<()> {
        let mut stack = lock(&self.stack)?;
        let before = stack.snapshot()?;
        if let Err(error) = stack.execute(
            ResourceAddress::new(ResourceKind::Queue, OUTBOX)?,
            CommandMetadata::default(),
            StackCommand::Queue(QueueCommand::Nack {
                receipt_handle: lease.receipt_handle.clone(),
                delay_ms: delay_milliseconds,
            }),
        ) {
            *stack = Stack::from_snapshot(before);
            return Err(error.into());
        }
        self.checkpoint(&mut stack, before)
    }

    /// Move one item directly to dead letters because a hard prerequisite can
    /// no longer complete.
    pub fn dead_letter_work(&self, lease: &WorkLease) -> Result<()> {
        self.finish_lease(lease, LeaseDisposition::DeadLetter)
    }

    /// Inspect durable causal markers for a webhook's hard prerequisites.
    pub fn dependency_readiness(&self, lease: &WorkLease) -> Result<DependencyReadiness> {
        let DeferredWork::Webhook {
            correlation_id,
            after,
            ..
        } = lease.work()
        else {
            return Ok(DependencyReadiness::Ready);
        };
        if after.is_empty() {
            return Ok(DependencyReadiness::Ready);
        }
        let stack = lock(&self.stack)?;
        let resource = ResourceAddress::new(ResourceKind::Db, WORK_STATE)?;
        let mut readiness = DependencyReadiness::Ready;
        for dependency in after {
            let outcome = stack.journal().entries().iter().rev().find(|entry| {
                entry.resource == resource
                    && entry.correlation_id.as_deref() == Some(correlation_id.as_str())
                    && entry.causation_id.as_deref() == Some(dependency.as_str())
                    && matches!(entry.outcome, JournalOutcome::Succeeded)
            });
            match outcome.map(|entry| entry.action.as_str()) {
                Some("put") => {}
                Some("delete") => return Ok(DependencyReadiness::Blocked),
                _ => readiness = DependencyReadiness::Waiting,
            }
        }
        Ok(readiness)
    }

    /// Lease exhausted work for operator inspection. Items remain available
    /// after the visibility timeout unless acknowledged through `redeliver`.
    pub fn dead_letters(&self, limit: usize) -> Result<Vec<DeadLetter>> {
        let receipt = self.execute_stack(
            ResourceAddress::new(ResourceKind::Queue, DEAD_LETTERS)?,
            CommandMetadata::default(),
            StackCommand::Queue(QueueCommand::Receive {
                max_messages: limit,
                visibility_timeout_ms: DEFAULT_VISIBILITY_TIMEOUT_MS,
            }),
        )?;
        match receipt.result {
            StackResult::Queue(result) => match *result {
                QueueResult::Messages(messages) => {
                    messages.into_iter().map(work::decode_dead_letter).collect()
                }
                _ => Err(Error::InvalidWork(
                    "dead-letter receive returned an unexpected result".to_owned(),
                )),
            },
            _ => Err(Error::InvalidWork(
                "dead-letter receive returned a non-queue result".to_owned(),
            )),
        }
    }

    /// Move one exhausted item back to the durable outbox with a fresh retry
    /// budget, then acknowledge its dead-letter record atomically.
    pub fn redeliver(&self, dead_letter: &DeadLetter) -> Result<()> {
        let mut stack = lock(&self.stack)?;
        let before = stack.snapshot()?;
        let result = (|| {
            stack.execute(
                ResourceAddress::new(ResourceKind::Queue, OUTBOX)?,
                CommandMetadata {
                    causation_id: Some(dead_letter.id().to_owned()),
                    operation_id: Some(dead_letter.work().operation().to_owned()),
                    ..CommandMetadata::default()
                },
                StackCommand::Queue(QueueCommand::Publish {
                    body: work::encode(&dead_letter.work().with_fresh_retry_budget())?,
                    attributes: work::attributes(dead_letter.work()),
                    delay_ms: 0,
                }),
            )?;
            stack.execute(
                ResourceAddress::new(ResourceKind::Queue, DEAD_LETTERS)?,
                CommandMetadata::default(),
                StackCommand::Queue(QueueCommand::Ack {
                    receipt_handle: dead_letter.lease.receipt_handle.clone(),
                }),
            )?;
            Ok::<(), Error>(())
        })();
        if let Err(error) = result {
            *stack = Stack::from_snapshot(before);
            return Err(error);
        }
        self.checkpoint(&mut stack, before)
    }

    /// Inspect queued and exhausted work without leasing or mutating it.
    pub fn inspect_deferred_work(&self) -> Result<DeferredWorkInspection> {
        let snapshot = lock(&self.stack)?.snapshot()?;
        let mut items = inspect_work_queue(&snapshot, OUTBOX, "outbox")?;
        let dead_letters = inspect_work_queue(&snapshot, DEAD_LETTERS, "dead_letter")?;
        let leased = items.iter().filter(|item| item.leased).count();
        let pending = items.len().saturating_sub(leased);
        let dead_letter_count = dead_letters.len();
        items.extend(dead_letters);
        Ok(DeferredWorkInspection {
            pending,
            leased,
            dead_letters: dead_letter_count,
            items,
        })
    }

    fn finish_lease(&self, lease: &WorkLease, disposition: LeaseDisposition) -> Result<()> {
        let mut stack = lock(&self.stack)?;
        let before = stack.snapshot()?;
        let result = (|| {
            if matches!(disposition, LeaseDisposition::Acknowledge) {
                record_webhook_outcome(&mut stack, lease, true)?;
            }
            if matches!(disposition, LeaseDisposition::DeadLetter) {
                stack.execute(
                    ResourceAddress::new(ResourceKind::Queue, DEAD_LETTERS)?,
                    CommandMetadata {
                        causation_id: Some(lease.id().to_owned()),
                        operation_id: Some(lease.work().operation().to_owned()),
                        ..CommandMetadata::default()
                    },
                    StackCommand::Queue(QueueCommand::Publish {
                        body: work::encode(lease.work())?,
                        attributes: BTreeMap::from([
                            ("attempts".to_owned(), lease.attempt().to_string()),
                            ("name".to_owned(), lease.work().name().to_owned()),
                        ]),
                        delay_ms: 0,
                    }),
                )?;
                record_webhook_outcome(&mut stack, lease, false)?;
            }
            if let LeaseDisposition::Retry(delay_ms) = disposition {
                let failed = lease.work().after_failure();
                stack.execute(
                    ResourceAddress::new(ResourceKind::Queue, OUTBOX)?,
                    CommandMetadata {
                        causation_id: Some(lease.id().to_owned()),
                        operation_id: Some(lease.work().operation().to_owned()),
                        ..CommandMetadata::default()
                    },
                    StackCommand::Queue(QueueCommand::Publish {
                        body: work::encode(&failed)?,
                        attributes: work::attributes(&failed),
                        delay_ms,
                    }),
                )?;
            }
            stack.execute(
                ResourceAddress::new(ResourceKind::Queue, OUTBOX)?,
                CommandMetadata::default(),
                StackCommand::Queue(QueueCommand::Ack {
                    receipt_handle: lease.receipt_handle.clone(),
                }),
            )?;
            Ok::<(), Error>(())
        })();
        if let Err(error) = result {
            *stack = Stack::from_snapshot(before);
            return Err(error);
        }
        self.checkpoint(&mut stack, before)
    }
}

fn inspect_work_queue(
    snapshot: &StackSnapshot,
    resource_name: &str,
    queue: &str,
) -> Result<Vec<InspectedDeferredWork>> {
    let address = ResourceAddress::new(ResourceKind::Queue, resource_name)?;
    let value = snapshot
        .resource_state(&address)
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
    let value = if value.is_null() {
        serde_json::Value::Array(Vec::new())
    } else {
        value
    };
    let messages = serde_json::from_value::<Vec<StoredWorkInspection>>(value)
        .map_err(|error| Error::InvalidWork(error.to_string()))?;
    messages
        .into_iter()
        .map(|message| {
            let work = serde_json::from_value(message.body)
                .map_err(|error| Error::InvalidWork(error.to_string()))?;
            Ok(InspectedDeferredWork {
                id: message.id,
                queue: queue.to_owned(),
                published_at: message.published_at,
                available_at: message.available_at,
                receive_count: message.receive_count,
                leased: message.receipt_handle.is_some(),
                work,
            })
        })
        .collect()
}

enum LeaseDisposition {
    Acknowledge,
    Retry(u64),
    DeadLetter,
}

fn record_webhook_outcome(stack: &mut Stack, lease: &WorkLease, completed: bool) -> Result<()> {
    let DeferredWork::Webhook {
        name,
        operation,
        correlation_id,
        ..
    } = lease.work()
    else {
        return Ok(());
    };
    let command = if completed {
        DbCommand::Put {
            collection: "webhook_deliveries".to_owned(),
            key: format!("{correlation_id}:{name}"),
            value: serde_json::json!({ "completed": true }),
            condition: WriteCondition::Any,
        }
    } else {
        DbCommand::Delete {
            collection: "webhook_deliveries".to_owned(),
            key: format!("{correlation_id}:{name}"),
            condition: WriteCondition::Any,
        }
    };
    stack.execute(
        ResourceAddress::new(ResourceKind::Db, WORK_STATE)?,
        CommandMetadata {
            operation_id: Some(operation.clone()),
            causation_id: Some(name.clone()),
            correlation_id: Some(correlation_id.clone()),
            ..CommandMetadata::default()
        },
        StackCommand::Db(command),
    )?;
    Ok(())
}
