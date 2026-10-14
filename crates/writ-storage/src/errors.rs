use thiserror::Error;
use writ_core::recovery::SaveLanding;
use writ_core::text_encoding::{EncodingLossy, NotInEncoding, TextEncoding};

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

    /// A file could not be replaced in one step.
    #[error("atomic write failed: {0}")]
    AtomicWrite(#[from] crate::atomic::AtomicWriteError),

    /// A notes-index table was missing and could not be recreated.
    #[error("index repair failed: {message}")]
    IndexRepair {
        /// Human-readable description of the repair failure.
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

    /// The note's file is reachable under more than one name, so replacing it
    /// would leave the other names on the old text (ADR-028 §5).
    ///
    /// A save writes a sibling and renames it into place, which breaks the
    /// link rather than following it. Refusing is the only answer that keeps
    /// the promise the user made when they linked the file.
    ///
    /// The wording here is for logs. What the editor says is chosen from the
    /// stable code the command puts in front of it (`ERR_HARD_LINKED`,
    /// `src-tauri/src/commands/buffer.rs`).
    #[error("{path} is one of {links} names for the same file")]
    HardLinkedDestination {
        /// The note's path.
        path: String,
        /// How many names point at the file.
        links: u64,
    },

    /// The note's file is marked read-only, so nothing was written and the
    /// file was left exactly as it was.
    ///
    /// The wording here is for logs; the editor reads `ERR_READ_ONLY_DESTINATION`.
    #[error("{path} cannot be written")]
    DestinationReadOnly {
        /// The note's path.
        path: String,
    },

    /// The folder the note lives in would not take the file a save writes
    /// before it renames, so nothing was written.
    ///
    /// Separate from [`StorageError::DestinationReadOnly`] because the person
    /// has to change the folder rather than the file, and because a folder
    /// that is unwritable for a moment becomes writable again, so the save is
    /// worth pressing twice.
    ///
    /// The wording here is for logs; the editor reads `ERR_FOLDER_NOT_WRITABLE`.
    #[error("the folder holding {path} cannot be written")]
    DestinationFolderNotWritable {
        /// The note's path.
        path: String,
    },

    /// A buffer was named that the store does not hold, so nothing was
    /// recorded for it.
    #[error("no file in the store has the id {id}")]
    BufferMissing {
        /// The id that was named.
        id: String,
    },

    /// The file is over the size Writ opens
    /// (`writ_core::file_ops::THRESHOLD_MAX_BYTES`), so none of it was read
    /// (ADR-019).
    ///
    /// The wording here is for logs; the editor reads `ERR_OPEN_TOO_LARGE`,
    /// the code an open past that size answers with.
    #[error("the file is {size_bytes} bytes, over the size Writ opens")]
    FileTooLarge {
        /// The file's size, from a stat.
        size_bytes: u64,
    },

    /// The note's text holds characters its file's encoding has no bytes
    /// for, so nothing was written (ADR-049).
    ///
    /// The editor reads `ERR_ENCODING_LOSSY` and names the characters from
    /// the code points the command puts after it.
    #[error(
        "{encoding} has no bytes for {}",
        writ_core::text_encoding::code_points(characters)
    )]
    EncodingLossy {
        /// The encoding the file is in.
        encoding: TextEncoding,
        /// The characters it cannot hold, in the order they first appear.
        characters: Vec<char>,
    },

    /// The file's bytes are not text in the encoding the note reads it in, so
    /// nothing was read (ADR-049).
    #[error("the file is not {encoding} text")]
    NotInEncoding {
        /// The encoding the note reads its file in.
        encoding: TextEncoding,
    },

    /// A version was asked for that the store does not hold.
    ///
    /// The entry was pruned between the panel listing it and the person
    /// choosing it, or the text it names is not on disk any more.
    #[error("version {id} is not in the store")]
    VersionMissing {
        /// The entry that was asked for.
        id: i64,
    },

    /// A version's text no longer matches the digest it was kept under.
    ///
    /// The file in the data directory was changed or damaged after it was
    /// written, so what it holds is not the text the entry names, and putting
    /// it back would put back something nobody wrote.
    #[error("version {id} does not hold the text it was kept with")]
    VersionDamaged {
        /// The entry that was asked for.
        id: i64,
    },

    /// A move of the notes folder to another disk found files whose bytes are
    /// not on this machine, and copied nothing.
    ///
    /// Copying one would download it, and a folder of them is a download the
    /// person did not ask for (ADR-028 §5).
    #[error("{count} files in the folder have not finished downloading, {first} among them")]
    FolderNotDownloaded {
        /// How many entries are not on this machine.
        count: usize,
        /// The name of the first one found.
        first: String,
    },

    /// A move of the notes folder to another disk needs more free space than
    /// the destination has, and copied nothing.
    #[error("the move needs {needed} bytes and the destination has {available}")]
    NotEnoughSpace {
        /// What the copy would take.
        needed: u64,
        /// What the destination has free.
        available: u64,
    },

    /// A note was asked to take a name with nothing in it.
    ///
    /// The wording here is for logs. What the editor says is written at the
    /// command that catches this (`src-tauri/src/commands/notes.rs`).
    #[error("a file name cannot be empty")]
    NoteNameEmpty,

    /// A note cannot take a name the folder already holds (ADR-028 section 3).
    ///
    /// Deduping instead would silently give the user a name they did not ask
    /// for; the collision is named so they can pick another.
    #[error("the name {name} is already taken in {}", folder.display())]
    NoteNameTaken {
        /// The file name, extension included, that is already there.
        name: String,
        /// The folder holding it.
        folder: std::path::PathBuf,
    },

    /// The archive folder and the notes folder are the same folder, or one
    /// holds the other, so emptying the archive would rename every note onto
    /// a deduped copy of itself.
    ///
    /// The two are distinct folders on every ordinary launch; this is what a
    /// hand-written config or an environment override can make of them.
    #[error("the archive {} and the folder {} are not separate folders", archive.display(), notes.display())]
    ArchiveHoldsNotes {
        /// The archive folder as resolved.
        archive: std::path::PathBuf,
        /// The notes folder as resolved.
        notes: std::path::PathBuf,
    },

    /// A note could not be handed to the operating system's trash.
    ///
    /// The row stays and the file stays: a note is never unlinked to work
    /// around this (ADR-028 section 3).
    #[error("{} could not be moved to the trash: {message}", path.display())]
    NoteTrash {
        /// The note that is still on disk.
        path: std::path::PathBuf,
        /// What the platform reported.
        message: String,
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

impl StorageError {
    /// What became of the text a save carried, when the save failed with this
    /// error (ADR-044).
    ///
    /// A refusal over a file that changed under Writ writes the text beside
    /// the note when it can, and that copy is a file holding it. Every other
    /// failure here left the text with the editor alone: a file that is not
    /// downloaded, read-only, hard-linked or missing, a folder that would not
    /// take the write, a volume that stopped answering, any other I/O error.
    pub fn save_landing(&self) -> SaveLanding {
        match self {
            StorageError::SourceChangedOnDisk {
                conflict_copy: Some(_),
                ..
            } => SaveLanding::WrittenBeside,
            _ => SaveLanding::NotLanded,
        }
    }
}

/// Shorthand for a result whose error arm is [`StorageError`].
pub type StorageResult<T> = Result<T, StorageError>;

impl From<EncodingLossy> for StorageError {
    fn from(error: EncodingLossy) -> Self {
        StorageError::EncodingLossy {
            encoding: error.encoding,
            characters: error.characters,
        }
    }
}

impl From<NotInEncoding> for StorageError {
    fn from(error: NotInEncoding) -> Self {
        StorageError::NotInEncoding {
            encoding: error.encoding,
        }
    }
}
