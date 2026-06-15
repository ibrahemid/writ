use thiserror::Error;

/// Crate-wide error type for `writ-storage` operations.
///
/// Every fallible operation in this crate returns [`StorageResult`],
/// whose error arm is this enum. Underlying errors from `rusqlite`,
/// `std::io`, `toml`, and `serde_json` are wrapped via `#[from]` so
/// callers can use `?` without manual conversion.
#[derive(Error, Debug)]
pub enum StorageError {
    /// A failure propagated from the SQLite driver.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// An underlying I/O error propagated from `std::io`.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A TOML deserialization failure.
    #[error("toml parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    /// A TOML serialization failure.
    #[error("toml serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    /// A JSON encode or decode failure.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// A schema migration could not be applied.
    #[error("migration failed: {message}")]
    Migration {
        /// Human-readable description of the migration failure.
        message: String,
    },

    /// The database and on-disk state disagree in a way that could not
    /// be reconciled automatically.
    #[error("consistency error: {message}")]
    Consistency {
        /// Human-readable description of the inconsistency.
        message: String,
    },

    /// The on-disk database was written by a newer build of Writ whose
    /// schema this binary does not understand.
    ///
    /// Opening it anyway would read newer rows through an older column
    /// layout and silently corrupt data, so the store refuses to proceed.
    #[error(
        "database schema version {db_version} is newer than this build supports ({binary_version}); \
         upgrade Writ to open it"
    )]
    SchemaTooNew {
        /// Highest `schema_version` recorded in the database file.
        db_version: i32,
        /// Highest migration version embedded in this binary.
        binary_version: i32,
    },
}

/// Shorthand for a result whose error arm is [`StorageError`].
pub type StorageResult<T> = Result<T, StorageError>;
