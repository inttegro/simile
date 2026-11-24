use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::fs::{self, File, OpenOptions};
use std::path::Path;
use std::str::FromStr;

use fs2::FileExt as _;
use redb::{Database, ReadableTable, TableDefinition, TableHandle, WriteTransaction};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use simile_spec::ResourceKind;

use super::{StateIdentity, StateStore, StateStoreError, StateStoreResult, lock_path};
use crate::stack::{StackRecords, replay_storage_key};
use crate::{JournalEntry, PROTOCOL_VERSION, ResourceAddress, StackSnapshot};

const STATE_FORMAT_VERSION: u16 = 2;
const CURRENT_KEY: &str = "current";

const METADATA: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata");
const RESOURCES: TableDefinition<&str, &[u8]> = TableDefinition::new("resources");
const REPLAYS: TableDefinition<&str, &[u8]> = TableDefinition::new("replays");
const JOURNAL: TableDefinition<u64, &[u8]> = TableDefinition::new("journal");
const COUNTERS: TableDefinition<&str, &[u8]> = TableDefinition::new("counters");
const CLOCK: TableDefinition<&str, &[u8]> = TableDefinition::new("clock");

const LEGACY_STATE: TableDefinition<&str, &[u8]> = TableDefinition::new("simile_stack_state");

/// Crash-safe `redb` store for one component process.
///
/// One write transaction synchronizes six purpose-specific tables. Values use
/// versioned CBOR while keys retain the identities operators need to reason
/// about resources, replay records, and journal sequence.
#[derive(Debug)]
pub struct RedbStateStore {
    database: Database,
    identity: Option<StateIdentity>,
    _lock: File,
}

impl RedbStateStore {
    pub fn open(path: impl AsRef<Path>) -> StateStoreResult<Self> {
        Self::open_inner(path.as_ref(), None)
    }

    pub fn open_with_identity(
        path: impl AsRef<Path>,
        identity: StateIdentity,
    ) -> StateStoreResult<Self> {
        Self::open_inner(path.as_ref(), Some(identity))
    }

    /// Remove one durable state database when no process owns it.
    ///
    /// The same sidecar lock used by [`Self::open`] makes service removal fail
    /// safely instead of unlinking state from a running emulator.
    pub fn remove(path: impl AsRef<Path>) -> StateStoreResult<bool> {
        let path = path.as_ref();
        // Why: removing a registered service that has never been started must not create a lock
        // file (or fail because the state directory does not exist).
        if !path.exists() {
            return Ok(false);
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
        let removed = if path.exists() {
            fs::remove_file(path)?;
            true
        } else {
            false
        };
        drop(lock);
        if lock_path.exists() {
            fs::remove_file(lock_path)?;
        }
        Ok(removed)
    }

    fn open_inner(path: &Path, identity: Option<StateIdentity>) -> StateStoreResult<Self> {
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
        create_tables(&transaction)?;
        transaction.commit().map_err(database_error)?;

        Ok(Self {
            database,
            identity,
            _lock: lock,
        })
    }

    fn load_normalized(&self) -> StateStoreResult<Option<(StackSnapshot, PersistedMetadata)>> {
        let transaction = self.database.begin_read().map_err(database_error)?;
        let metadata_table = transaction.open_table(METADATA).map_err(database_error)?;
        let Some(metadata) = metadata_table
            .get(CURRENT_KEY)
            .map_err(database_error)?
            .map(|value| decode::<PersistedMetadata>(value.value()))
            .transpose()?
        else {
            return Ok(None);
        };
        validate_metadata(&metadata, self.identity.as_ref())?;

        let resources = {
            let table = transaction.open_table(RESOURCES).map_err(database_error)?;
            table
                .iter()
                .map_err(database_error)?
                .map(|entry| {
                    let (key, value) = entry.map_err(database_error)?;
                    let address = ResourceAddress::from_str(key.value())
                        .map_err(|error| StateStoreError::Decode(error.to_string()))?;
                    Ok((address, decode(value.value())?))
                })
                .collect::<StateStoreResult<BTreeMap<_, _>>>()?
        };
        if resource_digest(resources.keys()) != metadata.resource_digest {
            return Err(StateStoreError::Decode(
                "persisted resource identities do not match metadata".to_owned(),
            ));
        }

        let replays = load_string_records(&transaction, REPLAYS)?;
        for (identity, record) in &replays {
            if replay_storage_key(record) != *identity {
                return Err(StateStoreError::Decode(format!(
                    "replay identity `{identity}` does not match its encoded record"
                )));
            }
        }
        let journal = {
            let table = transaction.open_table(JOURNAL).map_err(database_error)?;
            table
                .iter()
                .map_err(database_error)?
                .map(|entry| {
                    let (key, value) = entry.map_err(database_error)?;
                    let event = decode::<JournalEntry>(value.value())?;
                    if event.sequence != key.value() {
                        return Err(StateStoreError::Decode(format!(
                            "journal key {} does not match event sequence {}",
                            key.value(),
                            event.sequence
                        )));
                    }
                    Ok((key.value(), event))
                })
                .collect::<StateStoreResult<BTreeMap<_, _>>>()?
        };
        let counters = load_single_record(&transaction, COUNTERS, "counters")?;
        let clocks = load_single_record(&transaction, CLOCK, "clock")?;

        StackSnapshot::from_records(StackRecords {
            resources,
            replays,
            journal,
            counters,
            clocks,
        })
        .map(|snapshot| Some((snapshot, metadata)))
        .map_err(StateStoreError::Decode)
    }

    fn refresh_identity(&self, metadata: &PersistedMetadata) -> StateStoreResult<()> {
        let Some(identity) = self.identity.as_ref() else {
            return Ok(());
        };
        if metadata.service_identity.as_deref() == Some(identity.service_identity.as_str())
            && metadata.artifact_digest.as_deref() == Some(identity.artifact_digest.as_str())
        {
            return Ok(());
        }

        let mut refreshed = metadata.clone();
        refreshed.service_identity = Some(identity.service_identity.clone());
        refreshed.artifact_digest = Some(identity.artifact_digest.clone());
        let transaction = self.database.begin_write().map_err(database_error)?;
        put_single_record(&transaction, METADATA, &refreshed)?;
        transaction.commit().map_err(database_error)
    }

    fn load_legacy(&self) -> StateStoreResult<Option<StackSnapshot>> {
        let transaction = self.database.begin_read().map_err(database_error)?;
        let has_legacy_table = transaction
            .list_tables()
            .map_err(database_error)?
            .any(|table| table.name() == "simile_stack_state");
        if !has_legacy_table {
            return Ok(None);
        }
        let table = transaction
            .open_table(LEGACY_STATE)
            .map_err(database_error)?;
        let Some(value) = table.get(CURRENT_KEY).map_err(database_error)? else {
            return Ok(None);
        };
        let stored = decode::<LegacyStoredState>(value.value())?;
        if stored.format_version != 1 {
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

    fn save_snapshot(
        &self,
        snapshot: &StackSnapshot,
        remove_legacy_table: bool,
    ) -> StateStoreResult<()> {
        let records = snapshot.to_records();
        let metadata = PersistedMetadata::from_records(&records, self.identity.as_ref());
        let resources = encode_string_records(
            records
                .resources
                .iter()
                .map(|(address, state)| (address.to_string(), state)),
        )?;
        let replays = encode_string_records(
            records
                .replays
                .iter()
                .map(|(identity, record)| (identity.clone(), record)),
        )?;
        let journal = records
            .journal
            .iter()
            .map(|(sequence, event)| Ok((*sequence, encode(event)?)))
            .collect::<StateStoreResult<BTreeMap<_, _>>>()?;

        let transaction = self.database.begin_write().map_err(database_error)?;
        put_single_record(&transaction, METADATA, &metadata)?;
        sync_string_records(&transaction, RESOURCES, &resources)?;
        sync_string_records(&transaction, REPLAYS, &replays)?;
        sync_journal(&transaction, &journal)?;
        put_single_record(&transaction, COUNTERS, &records.counters)?;
        put_single_record(&transaction, CLOCK, &records.clocks)?;
        if remove_legacy_table {
            transaction
                .delete_table(LEGACY_STATE)
                .map_err(database_error)?;
        }
        transaction.commit().map_err(database_error)
    }
}

impl StateStore for RedbStateStore {
    fn load(&self) -> StateStoreResult<Option<StackSnapshot>> {
        if let Some((snapshot, metadata)) = self.load_normalized()? {
            self.refresh_identity(&metadata)?;
            return Ok(Some(snapshot));
        }
        let Some(snapshot) = self.load_legacy()? else {
            return Ok(None);
        };

        self.save_snapshot(&snapshot, true)?;
        Ok(Some(snapshot))
    }

    fn save(&self, snapshot: &StackSnapshot) -> StateStoreResult<()> {
        self.save_snapshot(snapshot, false)
    }

    fn is_durable(&self) -> bool {
        true
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedMetadata {
    format_version: u16,
    protocol_version: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    service_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    artifact_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    component_kind: Option<ResourceKind>,
    resource_digest: String,
}

impl PersistedMetadata {
    fn from_records(records: &StackRecords, identity: Option<&StateIdentity>) -> Self {
        let kinds = records
            .resources
            .keys()
            .map(|resource| resource.kind)
            .collect::<BTreeSet<_>>();
        let component_kind = kinds.first().copied().filter(|_| kinds.len() == 1);
        Self {
            format_version: STATE_FORMAT_VERSION,
            protocol_version: PROTOCOL_VERSION,
            service_identity: identity.map(|identity| identity.service_identity.clone()),
            artifact_digest: identity.map(|identity| identity.artifact_digest.clone()),
            component_kind,
            resource_digest: resource_digest(records.resources.keys()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyStoredState {
    format_version: u16,
    protocol_version: u16,
    snapshot: StackSnapshot,
}

fn create_tables(transaction: &WriteTransaction) -> StateStoreResult<()> {
    let _ = transaction.open_table(METADATA).map_err(database_error)?;
    let _ = transaction.open_table(RESOURCES).map_err(database_error)?;
    let _ = transaction.open_table(REPLAYS).map_err(database_error)?;
    let _ = transaction.open_table(JOURNAL).map_err(database_error)?;
    let _ = transaction.open_table(COUNTERS).map_err(database_error)?;
    let _ = transaction.open_table(CLOCK).map_err(database_error)?;
    Ok(())
}

fn validate_metadata(
    metadata: &PersistedMetadata,
    expected_identity: Option<&StateIdentity>,
) -> StateStoreResult<()> {
    if metadata.format_version != STATE_FORMAT_VERSION {
        return Err(StateStoreError::Format {
            found: metadata.format_version,
            supported: STATE_FORMAT_VERSION,
        });
    }
    if metadata.protocol_version != PROTOCOL_VERSION {
        return Err(StateStoreError::Protocol {
            found: metadata.protocol_version,
            supported: PROTOCOL_VERSION,
        });
    }
    if let Some(expected) = expected_identity {
        if metadata.service_identity.as_deref() != Some(expected.service_identity.as_str()) {
            return Err(StateStoreError::Identity {
                field: "service identity",
                expected: expected.service_identity.clone(),
            });
        }
        // Artifact digests are provenance, not a persistence compatibility boundary. A contract or
        // behavior edit can change the compiled artifact without changing Stack's versioned engine
        // state. After the snapshot decodes successfully, the store refreshes its provenance and
        // the owning service initializes any newly declared resources.
    }
    Ok(())
}

fn resource_digest<'a>(resources: impl IntoIterator<Item = &'a ResourceAddress>) -> String {
    let mut digest = Sha256::new();
    for resource in resources {
        digest.update(resource.to_string().as_bytes());
        digest.update(b"\0");
    }
    format!("{:x}", digest.finalize())
}

fn encode<T: Serialize>(value: &T) -> StateStoreResult<Vec<u8>> {
    let mut encoded = Vec::new();
    ciborium::ser::into_writer(value, &mut encoded)
        .map_err(|error| StateStoreError::Encode(error.to_string()))?;
    Ok(encoded)
}

fn decode<T: DeserializeOwned>(value: &[u8]) -> StateStoreResult<T> {
    ciborium::de::from_reader(value).map_err(|error| StateStoreError::Decode(error.to_string()))
}

fn encode_string_records<'a, T: Serialize + 'a>(
    records: impl IntoIterator<Item = (String, &'a T)>,
) -> StateStoreResult<BTreeMap<String, Vec<u8>>> {
    records
        .into_iter()
        .map(|(key, value)| Ok((key, encode(value)?)))
        .collect()
}

fn load_string_records<T: DeserializeOwned>(
    transaction: &redb::ReadTransaction,
    definition: TableDefinition<&str, &[u8]>,
) -> StateStoreResult<BTreeMap<String, T>> {
    let table = transaction.open_table(definition).map_err(database_error)?;
    table
        .iter()
        .map_err(database_error)?
        .map(|entry| {
            let (key, value) = entry.map_err(database_error)?;
            Ok((key.value().to_owned(), decode(value.value())?))
        })
        .collect()
}

fn load_single_record<T: DeserializeOwned>(
    transaction: &redb::ReadTransaction,
    definition: TableDefinition<&str, &[u8]>,
    label: &str,
) -> StateStoreResult<T> {
    let table = transaction.open_table(definition).map_err(database_error)?;
    let value = table
        .get(CURRENT_KEY)
        .map_err(database_error)?
        .ok_or_else(|| StateStoreError::Decode(format!("persisted {label} is missing")))?;
    decode(value.value())
}

fn put_single_record<T: Serialize>(
    transaction: &WriteTransaction,
    definition: TableDefinition<&str, &[u8]>,
    value: &T,
) -> StateStoreResult<()> {
    let encoded = encode(value)?;
    let mut table = transaction.open_table(definition).map_err(database_error)?;
    let unchanged = table
        .get(CURRENT_KEY)
        .map_err(database_error)?
        .is_some_and(|stored| stored.value() == encoded.as_slice());
    if !unchanged {
        table
            .insert(CURRENT_KEY, encoded.as_slice())
            .map_err(database_error)?;
    }
    Ok(())
}

fn sync_string_records(
    transaction: &WriteTransaction,
    definition: TableDefinition<&str, &[u8]>,
    desired: &BTreeMap<String, Vec<u8>>,
) -> StateStoreResult<()> {
    let mut table = transaction.open_table(definition).map_err(database_error)?;
    let stale = table
        .iter()
        .map_err(database_error)?
        .map(|entry| {
            entry
                .map(|(key, _)| key.value().to_owned())
                .map_err(database_error)
        })
        .collect::<StateStoreResult<Vec<_>>>()?;
    for key in stale {
        if !desired.contains_key(&key) {
            table.remove(key.as_str()).map_err(database_error)?;
        }
    }
    for (key, value) in desired {
        let unchanged = table
            .get(key.as_str())
            .map_err(database_error)?
            .is_some_and(|stored| stored.value() == value.as_slice());
        if !unchanged {
            table
                .insert(key.as_str(), value.as_slice())
                .map_err(database_error)?;
        }
    }
    Ok(())
}

fn sync_journal(
    transaction: &WriteTransaction,
    desired: &BTreeMap<u64, Vec<u8>>,
) -> StateStoreResult<()> {
    let mut table = transaction.open_table(JOURNAL).map_err(database_error)?;
    let stale = table
        .iter()
        .map_err(database_error)?
        .map(|entry| entry.map(|(key, _)| key.value()).map_err(database_error))
        .collect::<StateStoreResult<Vec<_>>>()?;
    for key in stale {
        if !desired.contains_key(&key) {
            table.remove(key).map_err(database_error)?;
        }
    }
    for (key, value) in desired {
        let unchanged = table
            .get(*key)
            .map_err(database_error)?
            .is_some_and(|stored| stored.value() == value.as_slice());
        if !unchanged {
            table
                .insert(*key, value.as_slice())
                .map_err(database_error)?;
        }
    }
    Ok(())
}

fn database_error(error: impl Display) -> StateStoreError {
    StateStoreError::Database(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    use redb::{Database, ReadableTableMetadata, TableHandle};
    use serde_json::json;
    use simile_spec::ResourceKind;

    use super::{
        CLOCK, COUNTERS, CURRENT_KEY, JOURNAL, LEGACY_STATE, LegacyStoredState, METADATA,
        PersistedMetadata, REPLAYS, RESOURCES, RedbStateStore, decode, encode,
    };
    use crate::{
        CommandMetadata, DbCommand, DbResult, LogicalTime, ResourceAddress, Stack, StackCommand,
        StackResult, StateIdentity, StateStore, StateStoreError,
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
            CommandMetadata {
                idempotency_key: Some("z-first".to_owned()),
                ..CommandMetadata::default()
            },
            StackCommand::Db(DbCommand::Put {
                collection: "products".to_owned(),
                key: "pr_123".to_owned(),
                value: json!({"name": "Beat pack"}),
                condition: Default::default(),
            }),
        )?;
        stack.execute(
            resource.clone(),
            CommandMetadata {
                idempotency_key: Some("a-second".to_owned()),
                ..CommandMetadata::default()
            },
            StackCommand::Db(DbCommand::Put {
                collection: "products".to_owned(),
                key: "pr_456".to_owned(),
                value: json!({"name": "Jingle pack"}),
                condition: Default::default(),
            }),
        )?;

        {
            let store = RedbStateStore::open(&path)?;
            store.save(&stack.snapshot()?)?;
        }
        let store = RedbStateStore::open(&path)?;
        let snapshot = store.load()?.ok_or("missing persisted snapshot")?;
        assert_eq!(snapshot, stack.snapshot()?);
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
    fn redb_round_trips_every_resource_kind() -> Result<(), Box<dyn std::error::Error>> {
        let path = temporary_path("resource-kinds");
        let resources = [
            ResourceKind::Db,
            ResourceKind::Cache,
            ResourceKind::Queue,
            ResourceKind::Mail,
            ResourceKind::ObjectStore,
            ResourceKind::EventBus,
            ResourceKind::Scheduler,
            ResourceKind::Http,
        ]
        .into_iter()
        .map(|kind| ResourceAddress::new(kind, "primary"))
        .collect::<Result<Vec<_>, _>>()?;
        let stack = Stack::builder(LogicalTime::UNIX_EPOCH)
            .resources(resources)?
            .build()?;

        {
            let store = RedbStateStore::open(&path)?;
            store.save(&stack.snapshot()?)?;
        }
        let store = RedbStateStore::open(&path)?;
        assert_eq!(store.load()?, Some(stack.snapshot()?));

        cleanup(&path);
        Ok(())
    }

    #[test]
    fn redb_uses_independently_addressed_tables() -> Result<(), Box<dyn std::error::Error>> {
        let path = temporary_path("tables");
        let first = ResourceAddress::new(ResourceKind::Db, "products")?;
        let second = ResourceAddress::new(ResourceKind::Db, "customers")?;
        let mut stack = Stack::builder(LogicalTime::UNIX_EPOCH)
            .resources([first.clone(), second])?
            .build()?;
        stack.execute(
            first,
            CommandMetadata {
                idempotency_key: Some("create-product".to_owned()),
                ..CommandMetadata::default()
            },
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
        let database = Database::open(&path)?;
        let transaction = database.begin_read()?;
        assert_eq!(transaction.open_table(METADATA)?.len()?, 1);
        assert_eq!(transaction.open_table(RESOURCES)?.len()?, 2);
        assert_eq!(transaction.open_table(REPLAYS)?.len()?, 1);
        assert_eq!(transaction.open_table(JOURNAL)?.len()?, 1);
        assert_eq!(transaction.open_table(COUNTERS)?.len()?, 1);
        assert_eq!(transaction.open_table(CLOCK)?.len()?, 1);
        let mut tables = transaction
            .list_tables()?
            .map(|table| table.name().to_owned())
            .collect::<Vec<_>>();
        tables.sort();
        assert_eq!(
            tables,
            [
                "clock",
                "counters",
                "journal",
                "metadata",
                "replays",
                "resources",
            ]
        );

        cleanup(&path);
        Ok(())
    }

    #[test]
    fn reset_removes_replay_and_journal_records() -> Result<(), Box<dyn std::error::Error>> {
        let path = temporary_path("reset");
        let resource = ResourceAddress::new(ResourceKind::Db, "products")?;
        let mut stack = Stack::builder(LogicalTime::UNIX_EPOCH)
            .resource(resource.kind, resource.name.clone())?
            .build()?;
        stack.execute(
            resource,
            CommandMetadata {
                idempotency_key: Some("create-product".to_owned()),
                ..CommandMetadata::default()
            },
            StackCommand::Db(DbCommand::Put {
                collection: "products".to_owned(),
                key: "pr_123".to_owned(),
                value: json!({"name": "Beat pack"}),
                condition: Default::default(),
            }),
        )?;

        let store = RedbStateStore::open(&path)?;
        store.save(&stack.snapshot()?)?;
        stack.reset();
        store.save(&stack.snapshot()?)?;
        let transaction = store.database.begin_read()?;
        assert!(transaction.open_table(REPLAYS)?.is_empty()?);
        assert!(transaction.open_table(JOURNAL)?.is_empty()?);

        cleanup(&path);
        Ok(())
    }

    #[test]
    fn version_one_snapshot_is_migrated_in_place() -> Result<(), Box<dyn std::error::Error>> {
        let path = temporary_path("migration");
        let resource = ResourceAddress::new(ResourceKind::Db, "products")?;
        let stack = Stack::builder(LogicalTime::UNIX_EPOCH)
            .resource(resource.kind, resource.name)?
            .build()?;
        let database = Database::create(&path)?;
        let transaction = database.begin_write()?;
        {
            let mut table = transaction.open_table(LEGACY_STATE)?;
            let encoded = encode(&LegacyStoredState {
                format_version: 1,
                protocol_version: crate::PROTOCOL_VERSION,
                snapshot: stack.snapshot()?,
            })?;
            table.insert(CURRENT_KEY, encoded.as_slice())?;
        }
        transaction.commit()?;
        drop(database);

        let store = RedbStateStore::open(&path)?;
        assert!(store.load()?.is_some());
        let transaction = store.database.begin_read()?;
        assert!(
            transaction
                .open_table(METADATA)?
                .get(CURRENT_KEY)?
                .is_some()
        );
        assert!(
            transaction
                .list_tables()?
                .all(|table| table.name() != "simile_stack_state")
        );

        cleanup(&path);
        Ok(())
    }

    #[test]
    fn service_identity_is_verified_and_artifact_digest_is_refreshable()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = temporary_path("identity");
        let resource = ResourceAddress::new(ResourceKind::Db, "products")?;
        let stack = Stack::builder(LogicalTime::UNIX_EPOCH)
            .resource(resource.kind, resource.name)?
            .build()?;
        {
            let store = RedbStateStore::open_with_identity(
                &path,
                StateIdentity::new("inttegro", "sha256:artifact-one"),
            )?;
            store.save(&stack.snapshot()?)?;
        }

        {
            let store = RedbStateStore::open_with_identity(
                &path,
                StateIdentity::new("stripe", "sha256:artifact-one"),
            )?;
            assert!(matches!(
                store.load(),
                Err(StateStoreError::Identity { .. })
            ));
        }

        {
            let store = RedbStateStore::open_with_identity(
                &path,
                StateIdentity::new("inttegro", "sha256:artifact-two"),
            )?;
            let Some(_snapshot) = store.load()? else {
                return Err(std::io::Error::other("persisted state is missing").into());
            };

            let transaction = store.database.begin_read()?;
            let table = transaction.open_table(METADATA)?;
            let Some(metadata) = table.get(CURRENT_KEY)? else {
                return Err(std::io::Error::other("persisted metadata is missing").into());
            };
            let metadata = decode::<PersistedMetadata>(metadata.value())?;
            assert_eq!(
                metadata.artifact_digest.as_deref(),
                Some("sha256:artifact-two")
            );
        }

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
