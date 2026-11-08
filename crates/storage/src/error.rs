use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database migration failed: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("file system error: {0}")]
    Io(std::io::Error),
    /// A stored value could not be decoded. Includes the column, never the value.
    #[error("stored data is invalid: {0}")]
    Corrupt(&'static str),
}

pub type StorageResult<T> = Result<T, StorageError>;
