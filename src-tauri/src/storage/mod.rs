mod actions;
mod conversations;
mod error;
mod migrations;
mod models;
mod retention;
mod settings;
mod tasks;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags};

pub use error::{StorageError, StorageResult};
pub use migrations::CURRENT_SCHEMA_VERSION;
pub use models::*;

const DATABASE_FILENAME: &str = "crowclaw.sqlite3";

/// Durable local CrowClaw storage.
///
/// The integration layer supplies an application-data directory. This module
/// never discovers or hardcodes a user-specific path.
pub struct Storage {
    connection: Mutex<Connection>,
    database_path: PathBuf,
}

impl Storage {
    pub fn open(app_data_directory: impl AsRef<Path>) -> StorageResult<Self> {
        let app_data_directory = app_data_directory.as_ref();
        fs::create_dir_all(app_data_directory)?;
        let database_path = app_data_directory.join(DATABASE_FILENAME);
        let mut connection = Connection::open_with_flags(
            &database_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
        )?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;",
        )?;
        migrations::migrate(&mut connection)?;

        Ok(Self {
            connection: Mutex::new(connection),
            database_path,
        })
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub fn schema_version(&self) -> StorageResult<u32> {
        let connection = self.connection()?;
        Ok(connection.pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    /// Flushes and closes SQLite, returning the integration-supplied database
    /// location so an uninstaller can remove its containing app-data directory.
    pub fn close(self) -> StorageResult<PathBuf> {
        let connection = self
            .connection
            .into_inner()
            .map_err(|_| StorageError::LockPoisoned)?;
        connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(StorageError::from)?;
        connection.close().map_err(|(_, error)| error)?;
        Ok(self.database_path)
    }

    pub(crate) fn connection(&self) -> StorageResult<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| StorageError::LockPoisoned)
    }
}

pub(crate) fn now_ms() -> StorageResult<i64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            StorageError::InvalidData(format!("system clock is before Unix epoch: {error}"))
        })?;
    i64::try_from(elapsed.as_millis()).map_err(|_| {
        StorageError::InvalidData("system timestamp exceeds SQLite integer range".into())
    })
}

pub(crate) fn require_non_empty(field: &str, value: &str) -> StorageResult<()> {
    if value.trim().is_empty() {
        Err(StorageError::InvalidData(format!(
            "{field} must not be empty"
        )))
    } else {
        Ok(())
    }
}

pub(crate) fn value_to_json(value: &serde_json::Value) -> StorageResult<String> {
    Ok(serde_json::to_string(value)?)
}

pub(crate) fn optional_value_to_json(
    value: Option<&serde_json::Value>,
) -> StorageResult<Option<String>> {
    value.map(value_to_json).transpose()
}

pub(crate) fn json_to_value(value: String) -> StorageResult<serde_json::Value> {
    Ok(serde_json::from_str(&value)?)
}

pub(crate) fn optional_json_to_value(
    value: Option<String>,
) -> StorageResult<Option<serde_json::Value>> {
    value.map(|value| json_to_value(value)).transpose()
}
