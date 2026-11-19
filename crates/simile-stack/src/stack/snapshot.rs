use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::supervisor::ReplayRecord;
use crate::cache::CacheEngine;
use crate::db::DbEngine;
use crate::event_bus::BusEngine;
use crate::http::HttpEngine;
use crate::mail::MailEngine;
use crate::object_store::ObjectEngine;
use crate::queue::QueueEngine;
use crate::scheduler::SchedulerEngine;
use crate::{ClockSnapshot, Journal, LogicalTime, ResourceAddress, ScheduledTask};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvanceReport {
    pub now: LogicalTime,
    pub expired_cache_entries: usize,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fired_tasks: BTreeMap<String, Vec<ScheduledTask>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StackSnapshot {
    pub(super) clock: ClockSnapshot,
    pub(super) initial_clock: ClockSnapshot,
    pub(super) resources: BTreeSet<ResourceAddress>,
    pub(super) db: DbEngine,
    pub(super) cache: CacheEngine,
    pub(super) queue: QueueEngine,
    pub(super) mail: MailEngine,
    pub(super) object_store: ObjectEngine,
    pub(super) event_bus: BusEngine,
    pub(super) scheduler: SchedulerEngine,
    pub(super) http: HttpEngine,
    pub(super) journal: Journal,
    pub(super) next_command: u64,
    pub(super) next_event: u64,
    pub(super) replays: Vec<ReplayRecord>,
}
