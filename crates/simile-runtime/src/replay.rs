use std::collections::HashMap;
use std::sync::Mutex;

use crate::{Error, OperationOutcome, ReplayIdentity, Result, ValidatedBody};

/// Owns committed operation outcomes and resolves replay identities atomically.
///
/// The producer runs while the store is locked so concurrent calls sharing an
/// identity cannot both commit different first outcomes. Only the service
/// outcome is retained; attempt-local faults are deliberately applied later.
#[derive(Debug, Default)]
pub(crate) struct ReplayStore {
    records: Mutex<HashMap<ReplayKey, ReplayRecord>>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum ReplayKey {
    Idempotency { operation: String, key: String },
    Resource { operation: String, id: String },
}

#[derive(Clone, Debug)]
struct ReplayRecord {
    request_body: Option<ValidatedBody>,
    outcome: OperationOutcome,
}

pub(crate) struct ReplayResolution {
    pub(crate) outcome: OperationOutcome,
    pub(crate) replayed: bool,
}

impl ReplayStore {
    pub(crate) fn resolve(
        &self,
        operation: &str,
        identity: ReplayIdentity<'_>,
        request_body: Option<ValidatedBody>,
        produce: impl FnOnce() -> Result<OperationOutcome>,
    ) -> Result<ReplayResolution> {
        let key = replay_key(operation, identity)?;
        let mut records = self
            .records
            .lock()
            .map_err(|_| Error::ReplayStateUnavailable)?;

        if let Some(record) = records.get(&key) {
            if matches!(key, ReplayKey::Idempotency { .. }) && record.request_body != request_body {
                return Err(Error::IdempotencyConflict {
                    operation: operation.to_owned(),
                });
            }
            return Ok(ReplayResolution {
                outcome: record.outcome.clone(),
                replayed: true,
            });
        }

        let outcome = produce()?;
        records.insert(
            key,
            ReplayRecord {
                request_body,
                outcome: outcome.clone(),
            },
        );
        Ok(ReplayResolution {
            outcome,
            replayed: false,
        })
    }
}

fn replay_key(operation: &str, identity: ReplayIdentity<'_>) -> Result<ReplayKey> {
    match identity {
        ReplayIdentity::IdempotencyKey(key) => {
            let key = key.trim();
            if key.is_empty() {
                return Err(Error::EmptyReplayIdentity);
            }
            Ok(ReplayKey::Idempotency {
                operation: operation.to_owned(),
                key: key.to_owned(),
            })
        }
        ReplayIdentity::Resource(id) => {
            if !operation.starts_with("GET ") {
                return Err(Error::ResourceReplayRequiresGet {
                    operation: operation.to_owned(),
                });
            }
            let id = id.trim();
            if id.is_empty() {
                return Err(Error::EmptyReplayIdentity);
            }
            Ok(ReplayKey::Resource {
                operation: operation.to_owned(),
                id: id.to_owned(),
            })
        }
    }
}
