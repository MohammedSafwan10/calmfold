//! SQLite persistence for Keepstone.
//!
//! One [`Store`] owns one connection. It is `Send`, so the app can move it to a
//! background thread later without API changes. All writes that touch more
//! than one table run in a transaction.

mod codec;
mod error;
mod events;
mod notes;
mod schema;
mod tasks;

use std::path::Path;

use rusqlite::Connection;

pub use error::{StorageError, StorageResult};
pub use notes::NoteSummary;
pub use tasks::{Progress, TaskCounts, TaskFilter};

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (or creates) the database at `path` and migrates it to the latest schema.
    pub fn open(path: &Path) -> StorageResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(StorageError::Io)?;
        }
        let conn = Connection::open(path)?;
        // WAL keeps reads fast while a write is in progress and survives crashes well.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    /// A private in-memory database. Used by tests and previews.
    pub fn open_in_memory() -> StorageResult<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> StorageResult<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        schema::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    /// Current schema version, for diagnostics.
    pub fn schema_version(&self) -> StorageResult<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }
}
