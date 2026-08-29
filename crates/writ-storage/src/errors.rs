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

    /// A workspace search matcher could not be built from the query.
    #[error("search error: {0}")]
    Search(String),

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

    /// The file changed on disk since Writ last read it, and the new bytes
    /// are not what is being written (ADR-028 §5).
    ///
    /// The wording here is for logs. What the editor says is chosen from the
    /// stable code the command puts in front of it
    /// (`ERR_FILE_CHANGED_ON_DISK`, `src-tauri/src/commands/buffer.rs`).
    #[error("the file changed on disk: {path}")]
    SourceChangedOnDisk {
        /// The note's path.
        path: String,
        /// SHA-256 of the bytes now on disk, lowercase hex.
        disk_hash: String,
        /// Where the text that was being written went instead, when the dated
        /// copy beside the note could be written. `None` means even that
        /// failed, and the caller is still holding the only copy of it.
        conflict_copy: Option<String>,
    },

    /// The file's bytes are not on this machine, so there is nothing to
    /// compare a save against and nothing to write over (ADR-028 §5).
    ///
    /// Reading an evicted iCloud file makes the provider daemon fetch it, so
    /// the guard asks before it reads and stops here instead.
    #[error("the file has not finished downloading: {path}")]
    SourceNotDownloaded {
        /// The note's path.
        path: String,
    },

    /// A `schema_meta` row holds a value that is not the shape its key
    /// requires, so the bookkeeping it carries cannot be trusted.
    #[error("the recorded value for {key} is not a number: {value}")]
    SchemaMetaValue {
        /// The `schema_meta` key whose row was read.
        key: String,
        /// The value the row holds.
        value: String,
    },

    /// A path could not be recorded in `schema_meta` because it is not valid
    /// UTF-8 and would not survive the round trip back to a path.
    #[error("the path {} cannot be recorded as text", path.display())]
    UnrecordablePath {
        /// The path that could not be recorded.
        path: std::path::PathBuf,
    },

    /// The copy of the database taken before the notes migration could not be
    /// written.
    #[error("the database could not be copied to {}", path.display())]
    RollbackCopyWrite {
        /// Where the copy was being written.
        path: std::path::PathBuf,
        /// The underlying failure.
        #[source]
        cause: rusqlite::Error,
    },

    /// A copy of the database was asked for on a connection that is inside a
    /// transaction, which cannot produce one.
    #[error("the database cannot be copied while a transaction is open")]
    RollbackCopyInTransaction,

    /// The copy of the database taken before the notes migration could not be
    /// deleted once it aged out.
    #[error("the copy of the database at {} could not be deleted", path.display())]
    RollbackCopyRemove {
        /// The copy that could not be deleted.
        path: std::path::PathBuf,
        /// The underlying failure.
        #[source]
        cause: std::io::Error,
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
