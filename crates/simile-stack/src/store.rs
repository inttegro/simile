use std::ffi::OsString;
use std::fmt::Display;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fs2::FileExt as _;
use redb::{Database, TableDefinition};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use simile_spec::ResourceKind;
use thiserror::Error;

use crate::{PROTOCOL_VERSION, ResourceAddress, StackSnapshot};

const STATE_FORMAT_VERSION: u16 = 1;
const STATE_KEY: &str = "current";
const STATE_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("simile_stack_state");

pub type StateStoreResult<T> = std::result::Result<T, StateStoreError>;

pub(crate) fn default_component_state_path<'a>(
    kind: ResourceKind,
    resources: impl IntoIterator<Item = &'a ResourceAddress>,
) -> StateStoreResult<PathBuf> {
    Ok(component_state_path(&simile_home()?, kind, resources))
}

fn component_state_path<'a>(
    simile_home: &Path,
    kind: ResourceKind,
    resources: impl IntoIterator<Item = &'a ResourceAddress>,
) -> PathBuf {
    let mut fingerprint = Sha256::new();
    fingerprint.update(b"simile-stack-state-v1\0");
    fingerprint.update(kind.to_string().as_bytes());
    let mut resources = resources
        .into_iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    resources.sort();
    for resource in resources {
        fingerprint.update(b"\0");
        fingerprint.update(resource.as_bytes());
    }
    let fingerprint = format!("{:x}", fingerprint.finalize());
    simile_home
        .join("state")
        .join("stack")
        .join(kind.to_string())
        .join(format!("{fingerprint}.redb"))
}

fn simile_home() -> StateStoreResult<PathBuf> {
    if let Some(path) = std::env::var_os("SIMILE_HOME").filter(|path| !path.is_empty()) {
        let path = PathBuf::from(path);
        return if path.is_absolute() {
            Ok(path)
        } else {
            Ok(std::env::current_dir()?.join(path))
        };
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .map(|home| home.join(".simile"))
        .ok_or(StateStoreError::Home)
}

/// Durable boundary for one complete Stack state.
///
/// Engines remain unaware of storage. The process checkpoints the supervisor's
/// complete state only after a transition has produced a coherent snapshot.
pub trait StateStore: Send + Sync {
    fn load(&self) -> StateStoreResult<Option<StackSnapshot>>;
    fn save(&self, snapshot: &StackSnapshot) -> StateStoreResult<()>;

    fn is_durable(&self) -> bool {
        false
    }
}

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

/// Crash-safe `redb` store for one component process.
///
/// Versioned CBOR keeps the persistence representation independent of redb's
/// table value types. A sidecar lock prevents redb versions without native
/// multi-process coordination from opening the same state file twice.
#[derive(Debug)]
pub struct RedbStateStore {
    database: Database,
    _lock: File,
}

impl RedbStateStore {
    pub fn open(path: impl AsRef<Path>) -> StateStoreResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }

        let lock_path = lock_path(path);
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)?;
        lock.try_lock_exclusive()
            .map_err(|source| StateStoreError::Locked {
                path: path.to_path_buf(),
                source,
            })?;

        let database = Database::create(path).map_err(database_error)?;
        let transaction = database.begin_write().map_err(database_error)?;
        {
            let _ = transaction
                .open_table(STATE_TABLE)
                .map_err(database_error)?;
        }
        transaction.commit().map_err(database_error)?;

        Ok(Self {
            database,
            _lock: lock,
        })
    }
}

impl StateStore for RedbStateStore {
    fn load(&self) -> StateStoreResult<Option<StackSnapshot>> {
        let transaction = self.database.begin_read().map_err(database_error)?;
        let table = transaction
            .open_table(STATE_TABLE)
            .map_err(database_error)?;
        let Some(value) = table.get(STATE_KEY).map_err(database_error)? else {
            return Ok(None);
        };
        let stored = ciborium::de::from_reader::<StoredState, _>(value.value())
            .map_err(|error| StateStoreError::Decode(error.to_string()))?;
        if stored.format_version != STATE_FORMAT_VERSION {
            return Err(StateStoreError::Format {
                found: stored.format_version,
                supported: STATE_FORMAT_VERSION,
            });
        }
        if stored.protocol_version != PROTOCOL_VERSION {
            return Err(StateStoreError::Protocol {
                found: stored.protocol_version,
                supported: PROTOCOL_VERSION,
            });
        }
        Ok(Some(stored.snapshot))
    }

    fn save(&self, snapshot: &StackSnapshot) -> StateStoreResult<()> {
        let stored = StoredState {
            format_version: STATE_FORMAT_VERSION,
            protocol_version: PROTOCOL_VERSION,
            snapshot: snapshot.clone(),
        };
        let mut encoded = Vec::new();
        ciborium::ser::into_writer(&stored, &mut encoded)
            .map_err(|error| StateStoreError::Encode(error.to_string()))?;

        let transaction = self.database.begin_write().map_err(database_error)?;
        {
            let mut table = transaction
                .open_table(STATE_TABLE)
                .map_err(database_error)?;
            table
                .insert(STATE_KEY, encoded.as_slice())
                .map_err(database_error)?;
        }
        transaction.commit().map_err(database_error)?;
        Ok(())
    }

    fn is_durable(&self) -> bool {
        true
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredState {
    format_version: u16,
    protocol_version: u16,
    snapshot: StackSnapshot,
}

#[derive(Debug, Error)]
pub enum StateStoreError {
    #[error("state storage failed: {0}")]
    Database(String),
    #[error("could not decode persisted state: {0}")]
    Decode(String),
    #[error("could not encode persisted state: {0}")]
    Encode(String),
    #[error("persisted state format {found} is incompatible with supported format {supported}")]
    Format { found: u16, supported: u16 },
    #[error("could not determine the user home directory; set SIMILE_HOME explicitly")]
    Home,
    #[error("state storage I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("state file `{path}` is already in use")]
    Locked {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the in-memory state store lock is unavailable")]
    Poisoned,
    #[error("persisted protocol {found} is incompatible with supported protocol {supported}")]
    Protocol { found: u16, supported: u16 },
}

fn database_error(error: impl Display) -> StateStoreError {
    StateStoreError::Database(error.to_string())
}

fn lock_path(path: &Path) -> PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(".lock");
    PathBuf::from(value)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;
    use simile_spec::ResourceKind;

    use super::{RedbStateStore, StateStore, StateStoreError, component_state_path};
    use crate::{
        DbCommand, DbResult, LogicalTime, ResourceAddress, Stack, StackCommand, StackResult,
    };

    #[test]
    fn redb_round_trips_complete_stack_state() -> Result<(), Box<dyn std::error::Error>> {
        let path = temporary_path("round-trip");
        let resource = ResourceAddress::new(ResourceKind::Db, "products")?;
        let mut stack = Stack::builder(LogicalTime::from_unix_millis(5))
            .resource(resource.kind, resource.name.clone())?
            .build()?;
        stack.execute(
            resource.clone(),
            Default::default(),
            StackCommand::Db(DbCommand::Put {
                collection: "products".to_owned(),
                key: "pr_123".to_owned(),
                value: json!({"name": "Beat pack"}),
                condition: Default::default(),
            }),
        )?;

        {
            let store = RedbStateStore::open(&path)?;
            store.save(&stack.snapshot()?)?;
        }
        let store = RedbStateStore::open(&path)?;
        let snapshot = store.load()?.ok_or("missing persisted snapshot")?;
        let mut restored = Stack::from_snapshot(snapshot);
        let result = restored.execute(
            resource,
            Default::default(),
            StackCommand::Db(DbCommand::Get {
                collection: "products".to_owned(),
                key: "pr_123".to_owned(),
            }),
        )?;
        assert!(matches!(
            result.result,
            StackResult::Db(value) if matches!(*value, DbResult::Document(Some(_)))
        ));

        cleanup(&path);
        Ok(())
    }

    #[test]
    fn redb_refuses_a_second_process_owner() -> Result<(), Box<dyn std::error::Error>> {
        let path = temporary_path("lock");
        let first = RedbStateStore::open(&path)?;
        let second = RedbStateStore::open(&path);
        assert!(matches!(second, Err(StateStoreError::Locked { .. })));
        drop(first);
        cleanup(&path);
        Ok(())
    }

    #[test]
    fn automatic_paths_are_stable_across_resource_argument_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let home = Path::new("simile-home");
        let first = ResourceAddress::new(ResourceKind::Queue, "fulfillment")?;
        let second = ResourceAddress::new(ResourceKind::Queue, "receipts")?;
        let forward = component_state_path(home, ResourceKind::Queue, [&first, &second]);
        let reverse = component_state_path(home, ResourceKind::Queue, [&second, &first]);

        assert_eq!(forward, reverse);
        assert!(forward.starts_with(home.join("state").join("stack").join("queue")));
        Ok(())
    }

    fn temporary_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        std::env::temp_dir().join(format!(
            "simile-stack-{label}-{}-{nonce}.redb",
            process::id()
        ))
    }

    fn cleanup(path: &Path) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(super::lock_path(path));
    }
}
