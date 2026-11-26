//! Versioned, loopback-only control protocol for the browser and terminal consoles.

mod execute;
mod http;
mod proof;
mod protocol;
mod replay;
mod session;

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde_json::Value;

pub(crate) use proof::{ExecutionProof, execution_proof};
pub(crate) use protocol::response;

use crate::inspection::unix_time_millis;

pub(super) const CONTROL_PREFIX: &str = "/.simile/console/v1";
const EVENT_LIMIT: usize = 256;

pub(super) struct ConsoleControl {
    next_event: AtomicU64,
    events: Mutex<VecDeque<ConsoleEvent>>,
    sessions: Mutex<BTreeMap<String, session::StoredSession>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConsoleEvent {
    sequence: u64,
    at_ms: u64,
    kind: String,
    summary: String,
    detail: Value,
}

impl ConsoleControl {
    pub(super) fn new() -> Self {
        Self {
            next_event: AtomicU64::new(0),
            events: Mutex::new(VecDeque::new()),
            sessions: Mutex::new(BTreeMap::new()),
        }
    }

    pub(super) fn record(&self, kind: &str, summary: impl Into<String>, detail: Value) {
        let sequence = self.next_event.fetch_add(1, Ordering::Relaxed) + 1;
        let event = ConsoleEvent {
            sequence,
            at_ms: unix_time_millis(),
            kind: kind.to_owned(),
            summary: summary.into(),
            detail,
        };
        let Ok(mut events) = self.events.lock() else {
            return;
        };
        events.push_back(event);
        while events.len() > EVENT_LIMIT {
            events.pop_front();
        }
    }
}
