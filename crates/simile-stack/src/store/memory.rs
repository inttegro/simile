use std::sync::Mutex;

use super::{StateStore, StateStoreError, StateStoreResult};
use crate::StackSnapshot;

/// Process-local store for embeddings and tests that deliberately avoid disk state.
#[derive(Debug, Default)]
pub struct MemoryStateStore {
    snapshot: Mutex<Option<StackSnapshot>>,
}

impl StateStore for MemoryStateStore {
    fn load(&self) -> StateStoreResult<Option<StackSnapshot>> {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .map_err(|_| StateStoreError::Poisoned)
    }

    fn save(&self, snapshot: &StackSnapshot) -> StateStoreResult<()> {
        *self
            .snapshot
            .lock()
            .map_err(|_| StateStoreError::Poisoned)? = Some(snapshot.clone());
        Ok(())
    }
}
