//! Process-local and crash-safe storage for sanitized HTTP exchange history.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fmt::Display;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fs2::FileExt as _;
use redb::{Database, ReadableTable, TableDefinition, WriteTransaction};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{RecordedExchange, RuntimeExchangeReport, RuntimeReportSummary};

const FORMAT_VERSION: u16 = 1;
const CURRENT_KEY: &str = "current";
const METADATA: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata");
const EXCHANGES: TableDefinition<&str, &[u8]> = TableDefinition::new("exchanges");

pub(super) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("exchange history at `{path}` is already in use")]
    Locked {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not access exchange history: {0}")]
    Io(#[from] std::io::Error),

    #[error("could not access the exchange history database: {0}")]
    Database(String),

    #[error("could not encode exchange history: {0}")]
    Encode(String),

    #[error("could not decode exchange history: {0}")]
    Decode(String),

    #[error(
        "exchange history uses format version {found}, but this Simile supports version {supported}"
    )]
    Format { found: u16, supported: u16 },

    #[error("the in-memory exchange history lock is unavailable")]
    Unavailable,
}

/// Storage chosen by the server lifecycle: ephemeral for one-shot runtimes and redb for services.
pub(super) enum ExchangeStore {
    Memory(Mutex<VecDeque<RecordedExchange>>),
    Durable(DurableExchangeStore),
}

impl ExchangeStore {
    pub(super) fn memory() -> Self {
        Self::Memory(Mutex::new(VecDeque::new()))
    }

    pub(super) fn durable(path: impl AsRef<Path>) -> Result<(Self, u64)> {
        let store = DurableExchangeStore::open(path.as_ref())?;
        let latest_sequence = store.metadata()?.latest_sequence;
        Ok((Self::Durable(store), latest_sequence))
    }

    pub(super) fn record(&self, sequence: u64, exchange: RecordedExchange) -> Result<()> {
        match self {
            Self::Memory(exchanges) => {
                exchanges
                    .lock()
                    .map_err(|_| Error::Unavailable)?
                    .push_front(exchange);
                Ok(())
            }
            Self::Durable(store) => store.record(sequence, exchange),
        }
    }

    pub(super) fn snapshot(&self) -> Result<Vec<RecordedExchange>> {
        match self {
            Self::Memory(exchanges) => Ok(exchanges
                .lock()
                .map_err(|_| Error::Unavailable)?
                .iter()
                .cloned()
                .collect()),
            Self::Durable(store) => store.snapshot(),
        }
    }

    pub(super) fn clear(&self) -> Result<usize> {
        match self {
            Self::Memory(exchanges) => {
                let mut exchanges = exchanges.lock().map_err(|_| Error::Unavailable)?;
                let removed = exchanges.len();
                exchanges.clear();
                Ok(removed)
            }
            Self::Durable(store) => store.clear(),
        }
    }

    pub(super) fn report_capture(
        &self,
        exchange_id: &str,
    ) -> Result<Option<RuntimeExchangeReport>> {
        self.recorded(exchange_id)
            .map(|recorded| recorded.and_then(|recorded| recorded.report_capture))
    }

    pub(super) fn report(&self, exchange_id: &str) -> Result<Option<RuntimeReportSummary>> {
        self.recorded(exchange_id)
            .map(|recorded| recorded.and_then(|recorded| recorded.report))
    }

    pub(super) fn set_report(&self, exchange_id: &str, report: RuntimeReportSummary) -> Result<()> {
        match self {
            Self::Memory(exchanges) => {
                if let Some(recorded) = exchanges
                    .lock()
                    .map_err(|_| Error::Unavailable)?
                    .iter_mut()
                    .find(|recorded| recorded.exchange.id == exchange_id)
                {
                    recorded.report = Some(report);
                }
                Ok(())
            }
            Self::Durable(store) => store.set_report(exchange_id, report),
        }
    }

    pub(super) fn update_report(&self, report: RuntimeReportSummary) -> Result<()> {
        match self {
            Self::Memory(exchanges) => {
                for recorded in &mut *exchanges.lock().map_err(|_| Error::Unavailable)? {
                    if recorded
                        .report
                        .as_ref()
                        .is_some_and(|prepared| prepared.id == report.id)
                    {
                        recorded.report = Some(report.clone());
                    }
                }
                Ok(())
            }
            Self::Durable(store) => store.update_report(report),
        }
    }

    fn recorded(&self, exchange_id: &str) -> Result<Option<RecordedExchange>> {
        match self {
            Self::Memory(exchanges) => Ok(exchanges
                .lock()
                .map_err(|_| Error::Unavailable)?
                .iter()
                .find(|recorded| recorded.exchange.id == exchange_id)
                .cloned()),
            Self::Durable(store) => store.recorded(exchange_id),
        }
    }
}

pub(super) struct DurableExchangeStore {
    database: Database,
    _lock: File,
}

impl DurableExchangeStore {
    fn open(path: &Path) -> Result<Self> {
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
        lock.try_lock_exclusive().map_err(|source| Error::Locked {
            path: path.to_path_buf(),
            source,
        })?;

        let database = Database::create(path).map_err(database_error)?;
        let transaction = database.begin_write().map_err(database_error)?;
        let _ = transaction.open_table(METADATA).map_err(database_error)?;
        let _ = transaction.open_table(EXCHANGES).map_err(database_error)?;
        transaction.commit().map_err(database_error)?;
        let store = Self {
            database,
            _lock: lock,
        };
        let _ = store.metadata()?;
        Ok(store)
    }

    fn metadata(&self) -> Result<Metadata> {
        let transaction = self.database.begin_read().map_err(database_error)?;
        let table = transaction.open_table(METADATA).map_err(database_error)?;
        let metadata = table
            .get(CURRENT_KEY)
            .map_err(database_error)?
            .map(|value| decode::<Metadata>(value.value()))
            .transpose()?
            .unwrap_or_default();
        metadata.validate()?;
        Ok(metadata)
    }

    fn record(&self, sequence: u64, exchange: RecordedExchange) -> Result<()> {
        let id = exchange.exchange.id.clone();
        let stored = encode(&StoredExchange {
            format_version: FORMAT_VERSION,
            sequence,
            exchange,
        })?;
        let transaction = self.database.begin_write().map_err(database_error)?;
        {
            let mut table = transaction.open_table(EXCHANGES).map_err(database_error)?;
            table
                .insert(id.as_str(), stored.as_slice())
                .map_err(database_error)?;
        }
        let latest_sequence = {
            let table = transaction.open_table(METADATA).map_err(database_error)?;
            table
                .get(CURRENT_KEY)
                .map_err(database_error)?
                .map(|value| decode::<Metadata>(value.value()))
                .transpose()?
                .unwrap_or_default()
                .latest_sequence
                .max(sequence)
        };
        write_metadata(
            &transaction,
            &Metadata {
                format_version: FORMAT_VERSION,
                latest_sequence,
            },
        )?;
        transaction.commit().map_err(database_error)
    }

    fn snapshot(&self) -> Result<Vec<RecordedExchange>> {
        let transaction = self.database.begin_read().map_err(database_error)?;
        let table = transaction.open_table(EXCHANGES).map_err(database_error)?;
        let mut exchanges = table
            .iter()
            .map_err(database_error)?
            .map(|entry| {
                let (_, value) = entry.map_err(database_error)?;
                decode_stored(value.value()).map(|stored| stored.exchange)
            })
            .collect::<Result<Vec<_>>>()?;
        exchanges.sort_by(|left, right| {
            right
                .exchange
                .started_at_ms
                .cmp(&left.exchange.started_at_ms)
                .then_with(|| right.exchange.id.cmp(&left.exchange.id))
        });
        Ok(exchanges)
    }

    fn clear(&self) -> Result<usize> {
        let transaction = self.database.begin_write().map_err(database_error)?;
        let removed = {
            let mut table = transaction.open_table(EXCHANGES).map_err(database_error)?;
            let keys = table
                .iter()
                .map_err(database_error)?
                .map(|entry| {
                    entry
                        .map(|(key, _)| key.value().to_owned())
                        .map_err(database_error)
                })
                .collect::<Result<Vec<_>>>()?;
            let removed = keys.len();
            for key in keys {
                table.remove(key.as_str()).map_err(database_error)?;
            }
            removed
        };
        transaction.commit().map_err(database_error)?;
        Ok(removed)
    }

    fn recorded(&self, exchange_id: &str) -> Result<Option<RecordedExchange>> {
        let transaction = self.database.begin_read().map_err(database_error)?;
        let table = transaction.open_table(EXCHANGES).map_err(database_error)?;
        table
            .get(exchange_id)
            .map_err(database_error)?
            .map(|value| decode_stored(value.value()).map(|stored| stored.exchange))
            .transpose()
    }

    fn set_report(&self, exchange_id: &str, report: RuntimeReportSummary) -> Result<()> {
        let Some(mut stored) = self.stored(exchange_id)? else {
            return Ok(());
        };
        stored.exchange.report = Some(report);
        self.replace(exchange_id, &stored)
    }

    fn update_report(&self, report: RuntimeReportSummary) -> Result<()> {
        let transaction = self.database.begin_write().map_err(database_error)?;
        let updates = {
            let table = transaction.open_table(EXCHANGES).map_err(database_error)?;
            table
                .iter()
                .map_err(database_error)?
                .filter_map(|entry| match entry {
                    Ok((key, value)) => match decode_stored(value.value()) {
                        Ok(mut stored)
                            if stored
                                .exchange
                                .report
                                .as_ref()
                                .is_some_and(|prepared| prepared.id == report.id) =>
                        {
                            stored.exchange.report = Some(report.clone());
                            Some(Ok((key.value().to_owned(), stored)))
                        }
                        Ok(_) => None,
                        Err(error) => Some(Err(error)),
                    },
                    Err(error) => Some(Err(database_error(error))),
                })
                .collect::<Result<Vec<_>>>()?
        };
        {
            let mut table = transaction.open_table(EXCHANGES).map_err(database_error)?;
            for (key, stored) in updates {
                let encoded = encode(&stored)?;
                table
                    .insert(key.as_str(), encoded.as_slice())
                    .map_err(database_error)?;
            }
        }
        transaction.commit().map_err(database_error)
    }

    fn stored(&self, exchange_id: &str) -> Result<Option<StoredExchange>> {
        let transaction = self.database.begin_read().map_err(database_error)?;
        let table = transaction.open_table(EXCHANGES).map_err(database_error)?;
        table
            .get(exchange_id)
            .map_err(database_error)?
            .map(|value| decode_stored(value.value()))
            .transpose()
    }

    fn replace(&self, exchange_id: &str, stored: &StoredExchange) -> Result<()> {
        let encoded = encode(stored)?;
        let transaction = self.database.begin_write().map_err(database_error)?;
        {
            let mut table = transaction.open_table(EXCHANGES).map_err(database_error)?;
            table
                .insert(exchange_id, encoded.as_slice())
                .map_err(database_error)?;
        }
        transaction.commit().map_err(database_error)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredExchange {
    format_version: u16,
    sequence: u64,
    exchange: RecordedExchange,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    format_version: u16,
    latest_sequence: u64,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            latest_sequence: 0,
        }
    }
}

impl Metadata {
    fn validate(&self) -> Result<()> {
        if self.format_version == FORMAT_VERSION {
            Ok(())
        } else {
            Err(Error::Format {
                found: self.format_version,
                supported: FORMAT_VERSION,
            })
        }
    }
}

fn decode_stored(value: &[u8]) -> Result<StoredExchange> {
    let stored = decode::<StoredExchange>(value)?;
    if stored.format_version != FORMAT_VERSION {
        return Err(Error::Format {
            found: stored.format_version,
            supported: FORMAT_VERSION,
        });
    }
    Ok(stored)
}

fn write_metadata(transaction: &WriteTransaction, metadata: &Metadata) -> Result<()> {
    let encoded = encode(metadata)?;
    let mut table = transaction.open_table(METADATA).map_err(database_error)?;
    table
        .insert(CURRENT_KEY, encoded.as_slice())
        .map_err(database_error)?;
    Ok(())
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut encoded = Vec::new();
    ciborium::ser::into_writer(value, &mut encoded)
        .map_err(|error| Error::Encode(error.to_string()))?;
    Ok(encoded)
}

fn decode<T: DeserializeOwned>(value: &[u8]) -> Result<T> {
    ciborium::de::from_reader(value).map_err(|error| Error::Decode(error.to_string()))
}

fn database_error(error: impl Display) -> Error {
    Error::Database(error.to_string())
}

fn lock_path(path: &Path) -> PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(".lock");
    PathBuf::from(value)
}
