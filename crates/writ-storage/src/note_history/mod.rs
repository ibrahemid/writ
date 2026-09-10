//! Where the texts a note used to hold are kept.
//!
//! Outside the notes folder, always. Sync carries the notes folder, so a
//! version store inside it would be uploaded, merged and conflicted like a
//! note; and a person who deletes the folder would delete the one thing that
//! could put it back (spec 470). So the texts live under Writ's own data
//! directory, addressed by the SHA-256 of their bytes and sharded on the
//! first byte of it, and the index that says which note held what when lives
//! in `history.db` — its own file, not `writ.db`. The note index is read by a
//! second process (ADR-031) and a table written on every save has no business
//! in it, nor does the free space a pruned table leaves behind.
//!
//! The policy is [`writ_core::note_history`]'s and none of it is decided
//! again here: what earns an entry, what a note is keyed by, and what has to
//! go. This module is the filesystem and the database.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};
use tracing::warn;
use writ_core::hash::{digest_from_hex, sha256_hex, Sha256Digest};
use writ_core::note_history::{
    is_versionable, prune_plan, should_capture, VersionFacts, VersionKey,
};
use writ_core::notes::identity::{FileIdentity, IdentityProbe};
use writ_core::notes::WriteOrigin;

use crate::errors::{StorageError, StorageResult};
use crate::maintenance::{checkpoint_truncate, read_stats, MaintenanceOutcome};
use crate::paths::relative_slug;

/// The folder the texts live in, under Writ's data directory.
const BLOB_DIR: &str = "history";

/// The index file, beside `writ.db` and never part of it.
const DB_FILE: &str = "history.db";

/// One text a note held, as the panel lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionEntry {
    /// What names this entry everywhere else.
    pub id: i64,
    /// When the text was captured.
    pub at: SystemTime,
    /// What the text costs in bytes.
    pub bytes: u64,
    /// SHA-256 of the text, as hex.
    pub hash: String,
}

/// What became of one text handed to the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kept {
    /// An entry of its own.
    Added(i64),
    /// The newest entry, which was inside the merge window and made by the
    /// same run of saves, now holds this text instead of the one it had.
    Merged(i64),
    /// Nothing. The store already holds this text, or the note is over the
    /// size ceiling, or it is not a note the store keys.
    Nothing,
}

impl Kept {
    /// The entry this text ended up in, if it ended up in one.
    pub fn entry(&self) -> Option<i64> {
        match self {
            Self::Added(id) | Self::Merged(id) => Some(*id),
            Self::Nothing => None,
        }
    }

    /// Whether this text earned an entry of its own.
    pub fn is_new_entry(&self) -> bool {
        matches!(self, Self::Added(_))
    }
}

/// What a pruning pass did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PruneOutcome {
    /// How many entries were retired.
    pub retired: usize,
    /// How many texts were deleted, which is fewer than `retired` whenever
    /// two entries held the same text.
    pub texts_deleted: usize,
    /// How many bytes those texts occupied.
    pub bytes_freed: u64,
    /// What the pass did to the database file itself.
    pub database: Option<MaintenanceOutcome>,
}

/// The texts of one Writ's notes, and the index over them.
///
/// One instance per data directory, held by the app. Nothing outside the app
/// process opens it: the `writ` command and the MCP server read `writ.db`
/// read-only and never come here, so one process writes this file.
pub struct NoteHistoryStore {
    conn: Mutex<Connection>,
    blobs: PathBuf,
    notes_root: RwLock<Option<PathBuf>>,
    probe: RwLock<Option<Arc<dyn IdentityProbe>>>,
}

impl NoteHistoryStore {
    /// Opens, and creates if it has to, the store under `writ_dir`.
    ///
    /// # Errors
    ///
    /// [`StorageError::Io`] when the folder for the texts cannot be created
    /// and [`StorageError::Database`] when the index cannot be opened.
    pub fn open(writ_dir: &Path) -> StorageResult<Self> {
        let blobs = writ_dir.join(BLOB_DIR);
        std::fs::create_dir_all(&blobs)?;
        let conn = crate::database::connection::open_database(&writ_dir.join(DB_FILE))?;
        create_schema(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            blobs,
            notes_root: RwLock::new(None),
            probe: RwLock::new(None),
        })
    }

    /// Tells the store which folder the notes are in.
    ///
    /// Every key is folder-relative, so the store has to be told, and told
    /// again when the folder moves. A store that has not been told keys
    /// nothing: [`Self::key_for`] answers `None` and every capture is a no-op,
    /// which is what a launch that has not resolved its notes folder yet
    /// should do rather than fill a store with keys it cannot use later.
    pub fn set_notes_root(&self, root: PathBuf) {
        match self.notes_root.write() {
            Ok(mut held) => *held = Some(root),
            Err(poisoned) => *poisoned.into_inner() = Some(root),
        }
    }

    /// Tells the store how to read what the filesystem calls a file.
    ///
    /// The platform's answer is the app's to give
    /// (`src-tauri/src/watcher/identity.rs`), so the store is handed it
    /// rather than reaching for a syscall of its own. A store with no probe
    /// keys every note by its path, which is the same answer a volume with no
    /// stable id gives.
    pub fn set_probe(&self, probe: Arc<dyn IdentityProbe>) {
        match self.probe.write() {
            Ok(mut held) => *held = Some(probe),
            Err(poisoned) => *poisoned.into_inner() = Some(probe),
        }
    }

    /// The key for the note at `path`, reading what the filesystem calls it.
    ///
    /// `None` for a file the notes folder does not hold. A file opened from
    /// somewhere else on the disk is not versioned: it is somebody else's
    /// file — a config, a log, a source file — and Writ keeping copies of it
    /// in its own data directory is not what opening a file in an editor asks
    /// for.
    pub fn key_for(&self, path: &Path) -> Option<VersionKey> {
        let identity = self.probe().and_then(|probe| probe.identity_of(path));
        self.key_at(path, identity)
    }

    /// The key for the note at `path`, as the caller already read it.
    ///
    /// For the one caller that knows an identity the filesystem will not
    /// answer for any more: a note whose file has just been deleted, whose id
    /// is about to be forgotten.
    pub fn key_at(&self, path: &Path, identity: Option<FileIdentity>) -> Option<VersionKey> {
        let root = self.notes_root()?;
        let slug = relative_slug(&root, path)?;
        Some(VersionKey::new(identity, PathBuf::from(slug)))
    }

    /// Keeps `bytes` as a version of the note `key` names, as `origin` wrote
    /// it.
    ///
    /// A text the editor wrote that
    /// [`writ_core::note_history::should_capture`] turned down inside the
    /// merge window becomes the newest entry rather than a second one
    /// ([`Kept::Merged`]), so a run of saves is one version of the note
    /// holding the last text the run wrote. Every other origin is an entry of
    /// its own or nothing at all ([`Merge::for_origin`]): a restore that
    /// merged into the save it landed on would sweep the text it was there to
    /// let somebody go back to.
    ///
    /// # Errors
    ///
    /// [`StorageError::Io`] when the text cannot be written and
    /// [`StorageError::Database`] when the index cannot be read or written.
    pub fn capture(
        &self,
        key: &VersionKey,
        bytes: &[u8],
        at: SystemTime,
        origin: &WriteOrigin,
    ) -> StorageResult<Kept> {
        self.record(key, bytes, at, Merge::for_origin(origin))
    }

    /// Keeps `bytes` as a version of the note `key` names, whatever the merge
    /// window says.
    ///
    /// For a text the file is about to stop holding: what a save is landing
    /// on, what a conflict resolution is setting aside, what a program
    /// outside Writ overwrote. The merge window exists to collapse a run of
    /// saves, each of which leaves its text in the file; a text with nowhere
    /// else to be has no successor to be merged into. The hash rule still
    /// applies, so a text the store already holds costs nothing.
    ///
    /// # Errors
    ///
    /// As [`Self::capture`].
    pub fn capture_replaced(
        &self,
        key: &VersionKey,
        bytes: &[u8],
        at: SystemTime,
    ) -> StorageResult<Kept> {
        self.record(key, bytes, at, Merge::Never)
    }

    /// The versions of the note `key` names, newest first.
    ///
    /// # Errors
    ///
    /// [`StorageError::Database`] when the index cannot be read.
    pub fn versions(&self, key: &VersionKey) -> StorageResult<Vec<VersionEntry>> {
        let conn = self.conn();
        // Reading a note's versions can write one row. `resolve_note` asks the
        // path first and, when the path has moved, repairs the note's row from
        // its durable identity — which is how a note renamed while nothing was
        // watching still lists what it held under its old name. The repair has
        // to happen on whichever call arrives first, and for a note nobody
        // saves again that call is this one. So this read takes the store's
        // write lock like every other call; one process holds it (`set_history`
        // has a single caller) and the panel asks once per open.
        let Some(note) = resolve_note(&conn, key, Mint::No)? else {
            return Ok(Vec::new());
        };
        let mut statement = conn.prepare(
            "SELECT id, at_ms, bytes, hash FROM versions \
             WHERE note_id = ?1 ORDER BY at_ms DESC, id DESC",
        )?;
        let rows = statement.query_map([note], |row| {
            Ok(VersionEntry {
                id: row.get(0)?,
                at: from_millis(row.get(1)?),
                bytes: row.get::<_, i64>(2)?.max(0) as u64,
                hash: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The text of one entry.
    ///
    /// # Errors
    ///
    /// [`StorageError::VersionMissing`] when the index does not hold the
    /// entry or the text it names is not on disk, and [`StorageError::Io`]
    /// when the text cannot be read.
    pub fn content(&self, version_id: i64) -> StorageResult<Vec<u8>> {
        let hash: Option<String> = self
            .conn()
            .query_row(
                "SELECT hash FROM versions WHERE id = ?1",
                [version_id],
                |row| row.get(0),
            )
            .optional()?;
        let hash = hash.ok_or(StorageError::VersionMissing { id: version_id })?;
        let path = self.blob_path(&hash);
        if !path.exists() {
            return Err(StorageError::VersionMissing { id: version_id });
        }
        Ok(std::fs::read(path)?)
    }

    /// Where the note an entry belongs to sits inside the notes folder.
    ///
    /// Folder-relative, like every path this module holds. The caller joins
    /// it onto the notes folder it is working against, so a store carried to
    /// another machine names notes rather than somebody's home directory.
    ///
    /// # Errors
    ///
    /// [`StorageError::VersionMissing`] when the index does not hold the
    /// entry.
    pub fn note_of(&self, version_id: i64) -> StorageResult<PathBuf> {
        let path: Option<String> = self
            .conn()
            .query_row(
                "SELECT notes.path FROM versions \
                 JOIN notes ON notes.id = versions.note_id WHERE versions.id = ?1",
                [version_id],
                |row| row.get(0),
            )
            .optional()?;
        path.map(PathBuf::from)
            .ok_or(StorageError::VersionMissing { id: version_id })
    }

    /// Applies the retention policy and reclaims what it retired.
    ///
    /// Called on the schedule the app already keeps for its databases
    /// (`crates/writ-core/src/maintenance.rs`), never on a save: retention is
    /// a background cost and a save is the one moment the user is waiting.
    ///
    /// # Errors
    ///
    /// [`StorageError::Database`] when the index cannot be read or written.
    /// A text that cannot be deleted is logged and the pass carries on: an
    /// entry is gone from the index either way, and a file left behind is
    /// picked up by the next pass.
    pub fn prune(&self, now: SystemTime) -> StorageResult<PruneOutcome> {
        let mut conn = self.conn();
        let facts = read_facts(&conn)?;
        let plan = prune_plan(&facts, now);
        if plan.is_empty() {
            return Ok(PruneOutcome {
                retired: 0,
                texts_deleted: 0,
                bytes_freed: 0,
                database: None,
            });
        }

        // One transaction, dropped rather than committed on the way out of
        // any `?`: a pass that stops halfway leaves the index as it was, and
        // the texts are swept afterwards from what the commit actually says.
        let mut orphaned: HashSet<String> = HashSet::new();
        {
            let transaction = conn.transaction()?;
            {
                let mut statement =
                    transaction.prepare("SELECT hash FROM versions WHERE id = ?1")?;
                let mut delete = transaction.prepare("DELETE FROM versions WHERE id = ?1")?;
                for id in &plan.retire {
                    let hash: Option<String> =
                        statement.query_row([id], |row| row.get(0)).optional()?;
                    delete.execute([id])?;
                    if let Some(hash) = hash {
                        orphaned.insert(hash);
                    }
                }
            }
            transaction.execute(
                "DELETE FROM notes WHERE id NOT IN (SELECT DISTINCT note_id FROM versions)",
                [],
            )?;
            transaction.commit()?;
        }

        let mut texts_deleted = 0;
        let mut bytes_freed = 0;
        for hash in orphaned {
            let still_held: Option<i64> = conn
                .query_row(
                    "SELECT 1 FROM versions WHERE hash = ?1 LIMIT 1",
                    [&hash],
                    |row| row.get(0),
                )
                .optional()?;
            if still_held.is_some() {
                continue;
            }
            let path = self.blob_path(&hash);
            let size = path.metadata().map(|meta| meta.len()).unwrap_or(0);
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    texts_deleted += 1;
                    bytes_freed += size;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => warn!(error = %e, "a retired version's text could not be deleted"),
            }
        }

        // The index is a table written on every save and pruned on a timer,
        // which is the shape that leaves a file of free pages behind
        // (`writ_core::maintenance`).
        let before = read_stats(&conn)?;
        let vacuumed =
            writ_core::maintenance::needs_vacuum(before.page_count, before.freelist_count);
        if vacuumed {
            conn.execute_batch("VACUUM")?;
        }
        checkpoint_truncate(&conn)?;
        let after = read_stats(&conn)?;

        Ok(PruneOutcome {
            retired: plan.retire.len(),
            texts_deleted,
            bytes_freed,
            database: Some(MaintenanceOutcome {
                before,
                after,
                vacuumed,
            }),
        })
    }

    /// The probe the app handed over, when it handed one over.
    fn probe(&self) -> Option<Arc<dyn IdentityProbe>> {
        match self.probe.read() {
            Ok(held) => held.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// The notes folder the store keys against, when it has been told one.
    fn notes_root(&self) -> Option<PathBuf> {
        match self.notes_root.read() {
            Ok(held) => held.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// The one place a capture is written, whichever rule brought it here.
    fn record(
        &self,
        key: &VersionKey,
        bytes: &[u8],
        at: SystemTime,
        merge: Merge,
    ) -> StorageResult<Kept> {
        if !is_versionable(bytes.len() as u64) {
            return Ok(Kept::Nothing);
        }
        let hash = sha256_hex(bytes);
        let Some(digest) = digest_from_hex(&hash) else {
            return Ok(Kept::Nothing);
        };
        let conn = self.conn();
        let Some(note) = resolve_note(&conn, key, Mint::Yes)? else {
            return Ok(Kept::Nothing);
        };
        let newest: Option<Newest> = conn
            .query_row(
                "SELECT id, at_ms, hash, merges FROM versions WHERE note_id = ?1 \
                 ORDER BY at_ms DESC, id DESC LIMIT 1",
                [note],
                |row| {
                    Ok(Newest {
                        id: row.get(0)?,
                        at_ms: row.get(1)?,
                        hash: row.get(2)?,
                        merges: row.get::<_, i64>(3)? != 0,
                    })
                },
            )
            .optional()?;
        let last_hash: Option<Sha256Digest> = newest
            .as_ref()
            .and_then(|newest| digest_from_hex(&newest.hash));
        // The window is measured against the run of saves this one would join,
        // never against an entry the external-change seam made: a save two
        // seconds after a note was opened is the first save of a run, not the
        // second text of one, and merging it into what the file held before
        // anybody typed would leave the save nowhere.
        let last_at = match merge {
            Merge::Window => newest
                .as_ref()
                .filter(|newest| newest.merges)
                .map(|newest| from_millis(newest.at_ms)),
            Merge::Never => None,
        };
        if !should_capture(last_at, at, digest, last_hash) {
            // Inside the window, and the newest entry is this same run of
            // saves: it becomes the text this save wrote. Dropping the text
            // instead would leave the run's latest work nowhere but the file,
            // and something else writing that file a second later would take
            // it. The entry keeps the time it was made, so the window is the
            // window and not a rolling one a long session never closes.
            let mergeable = newest.as_ref().is_some_and(|newest| newest.merges);
            if merge == Merge::Window && mergeable && last_hash != Some(digest) {
                let newest = newest.expect("mergeable implies an entry");
                self.write_blob(&hash, bytes)?;
                conn.execute(
                    "UPDATE versions SET hash = ?1, bytes = ?2 WHERE id = ?3",
                    rusqlite::params![&hash, bytes.len() as i64, newest.id],
                )?;
                self.forget_text(&conn, &newest.hash)?;
                return Ok(Kept::Merged(newest.id));
            }
            return Ok(Kept::Nothing);
        }

        // The text lands before the row that names it. A text with no row is
        // reclaimed by the next pass; a row naming a text that is not there
        // is an entry that cannot be restored.
        self.write_blob(&hash, bytes)?;
        conn.execute(
            "INSERT INTO versions (note_id, at_ms, bytes, hash, merges) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                note,
                as_millis(at),
                bytes.len() as i64,
                &hash,
                i64::from(merge == Merge::Window)
            ],
        )?;
        Ok(Kept::Added(conn.last_insert_rowid()))
    }

    /// Deletes a text no entry names any more.
    ///
    /// Called where one entry stops naming it — a merge — rather than only
    /// from [`Self::prune`], so a run of saves does not leave one file per
    /// keystroke behind for a pass that may be days away.
    fn forget_text(&self, conn: &Connection, hash: &str) -> StorageResult<()> {
        let still_held: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM versions WHERE hash = ?1 LIMIT 1",
                [hash],
                |row| row.get(0),
            )
            .optional()?;
        if still_held.is_some() {
            return Ok(());
        }
        match std::fs::remove_file(self.blob_path(hash)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => warn!(error = %e, "a text no version names could not be deleted"),
        }
        Ok(())
    }

    /// Where the text with this digest is kept.
    ///
    /// Sharded on the first byte so no directory holds every text: a folder
    /// of a hundred thousand entries is slow to list on every filesystem and
    /// unusable on some.
    fn blob_path(&self, hash: &str) -> PathBuf {
        let (shard, rest) = hash.split_at(2);
        self.blobs.join(shard).join(rest)
    }

    /// Writes a text, unless the store already holds it.
    fn write_blob(&self, hash: &str, bytes: &[u8]) -> StorageResult<()> {
        let path = self.blob_path(hash);
        if path.exists() {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::atomic::write_atomic(&path, bytes).map_err(|e| match e {
            crate::atomic::AtomicWriteError::Io(e) => StorageError::Io(e),
            other => StorageError::Consistency {
                message: format!("a version's text could not be written: {other}"),
            },
        })
    }

    /// The index connection, poisoning and all.
    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The newest entry a note has, which is what a capture is weighed against.
struct Newest {
    id: i64,
    at_ms: i64,
    hash: String,
    /// Whether a save inside the window may become this entry.
    ///
    /// True for the entries a run of saves makes and false for the ones the
    /// external-change seam makes: a text Writ did not write is not part of
    /// anybody's run of saves and must not be written over by one.
    merges: bool,
}

/// Whether the merge window applies to one capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Merge {
    /// The ordinary rule: a text captured inside the window is merged into
    /// the entry before it.
    Window,
    /// For a text that is about to stop existing, and for every write that is
    /// not the editor's own.
    Never,
}

impl Merge {
    /// Which writes a run of saves is allowed to absorb: the editor's, and
    /// nothing else.
    ///
    /// Merging replaces the newest entry's text and sweeps the one it held,
    /// which is right for the next keystroke of a run somebody is typing and
    /// wrong for everything else. A restore, an applied proposal, a program's
    /// write and a rename all land on a note the person may have saved
    /// seconds earlier, and merging one of those into that save would take
    /// the saved text out of the store — the one text the restore is there to
    /// go back to.
    ///
    /// Autosave and the save keystroke both reach here as
    /// [`WriteOrigin::Editor`] (`crate::buffer_store`), so the run this
    /// collapses is the whole of it.
    fn for_origin(origin: &WriteOrigin) -> Self {
        match origin {
            WriteOrigin::Editor => Merge::Window,
            WriteOrigin::Autosave
            | WriteOrigin::Restore
            | WriteOrigin::Chat
            | WriteOrigin::Cli
            | WriteOrigin::Mcp { .. }
            | WriteOrigin::RenamePropagation => Merge::Never,
        }
    }
}

/// Whether a note with no row yet gets one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mint {
    Yes,
    No,
}

/// Creates the schema, which is two tables and never changes shape in place.
///
/// `IF NOT EXISTS` rather than the migration framework `writ.db` uses: this
/// file is Writ's own cache of texts, and a schema change that cannot be
/// applied is answered by rebuilding it rather than by migrating a user's
/// data. Nothing here is the only copy of anything.
fn create_schema(conn: &Connection) -> StorageResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS notes (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            identity TEXT,
            birth_ns TEXT
        );
        CREATE TABLE IF NOT EXISTS versions (
            id INTEGER PRIMARY KEY,
            note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
            at_ms INTEGER NOT NULL,
            bytes INTEGER NOT NULL,
            hash TEXT NOT NULL,
            merges INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS versions_by_note ON versions(note_id, at_ms DESC, id DESC);
        CREATE INDEX IF NOT EXISTS versions_by_hash ON versions(hash);
        CREATE UNIQUE INDEX IF NOT EXISTS notes_by_identity
            ON notes(identity) WHERE identity IS NOT NULL;",
    )?;
    Ok(())
}

/// Finds the note row `key` names, and mints one when asked to.
///
/// The path is asked first and the identity second, because Writ's own saves
/// change the identity of every note: an atomic replace writes a sibling and
/// renames it over the note, so the file behind a path is a new file after
/// every save. What the identity is for is the other direction — a note
/// renamed inside the notes folder keeps its history, which is the Obsidian
/// limitation this beats (spec 486).
///
/// A [`FileIdentity::Fallback`] is recorded as no identity at all
/// ([`VersionKey::durable_identity`]), so a volume with no stable id keys its
/// notes by path, which is what every editor does and what this degrades to
/// rather than guessing.
fn resolve_note(conn: &Connection, key: &VersionKey, mint: Mint) -> StorageResult<Option<i64>> {
    let path = key.path.to_string_lossy().into_owned();
    let identity = key.durable_identity().map(encode_identity);

    let by_path: Option<i64> = conn
        .query_row("SELECT id FROM notes WHERE path = ?1", [&path], |row| {
            row.get(0)
        })
        .optional()?;
    if let Some(id) = by_path {
        if let Some((name, birth)) = &identity {
            claim_identity(conn, id, name, birth.as_deref())?;
        }
        return Ok(Some(id));
    }

    if let Some((name, birth)) = &identity {
        let found: Option<(i64, Option<String>)> = conn
            .query_row(
                "SELECT id, birth_ns FROM notes WHERE identity = ?1",
                [name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((id, recorded_birth)) = found {
            let recorded = decode_identity(name, recorded_birth.as_deref());
            let seen = key.durable_identity().cloned();
            if let (Some(recorded), Some(seen)) = (recorded, seen) {
                if seen.is_same_file(&recorded) {
                    conn.execute(
                        "UPDATE notes SET path = ?1 WHERE id = ?2",
                        rusqlite::params![&path, id],
                    )?;
                    claim_identity(conn, id, name, birth.as_deref())?;
                    return Ok(Some(id));
                }
            }
            // The id came back on a different file — a freed inode handed
            // out again. The row that held it keeps its history under its own
            // path and gives the id up.
            conn.execute(
                "UPDATE notes SET identity = NULL, birth_ns = NULL WHERE id = ?1",
                [id],
            )?;
        }
    }

    if mint == Mint::No {
        return Ok(None);
    }
    let (name, birth) = match identity {
        Some((name, birth)) => (Some(name), birth),
        None => (None, None),
    };
    conn.execute(
        "INSERT INTO notes (path, identity, birth_ns) VALUES (?1, ?2, ?3)",
        rusqlite::params![&path, &name, &birth],
    )?;
    Ok(Some(conn.last_insert_rowid()))
}

/// Puts an identity on one note row, taking it off whatever row held it.
///
/// The column is unique, so the row that held the id before has to give it up
/// first. It keeps every entry it has: what changed is which file answers to
/// its id, not what the note used to say.
fn claim_identity(
    conn: &Connection,
    note: i64,
    identity: &str,
    birth: Option<&str>,
) -> StorageResult<()> {
    conn.execute(
        "UPDATE notes SET identity = NULL, birth_ns = NULL WHERE identity = ?1 AND id <> ?2",
        rusqlite::params![identity, note],
    )?;
    conn.execute(
        "UPDATE notes SET identity = ?1, birth_ns = ?2 WHERE id = ?3",
        rusqlite::params![identity, birth, note],
    )?;
    Ok(())
}

/// What a durable identity is written as, and its birth time beside it.
///
/// The birth time is a column of its own rather than part of the name,
/// because [`FileIdentity::is_same_file`] lets an unknown one match a known
/// one and a single string could not express that.
fn encode_identity(identity: &FileIdentity) -> (String, Option<String>) {
    match identity {
        FileIdentity::Inode { dev, ino, birth_ns } => (
            format!("inode:{dev}:{ino}"),
            birth_ns.map(|born| born.to_string()),
        ),
        FileIdentity::Windows {
            volume,
            index,
            birth_ns,
        } => (
            format!("windows:{volume}:{index}"),
            birth_ns.map(|born| born.to_string()),
        ),
        // Never reached: a fallback identity is not durable and
        // `durable_identity` filters it out before this is called.
        FileIdentity::Fallback { path, .. } => (format!("fallback:{path}"), None),
    }
}

/// Reads back what [`encode_identity`] wrote.
fn decode_identity(name: &str, birth: Option<&str>) -> Option<FileIdentity> {
    let birth_ns = birth.and_then(|born| born.parse::<u128>().ok());
    let mut parts = name.split(':');
    match (parts.next()?, parts.next()?, parts.next()?) {
        ("inode", dev, ino) => Some(FileIdentity::Inode {
            dev: dev.parse().ok()?,
            ino: ino.parse().ok()?,
            birth_ns,
        }),
        ("windows", volume, index) => Some(FileIdentity::Windows {
            volume: volume.parse().ok()?,
            index: index.parse().ok()?,
            birth_ns,
        }),
        _ => None,
    }
}

/// Every entry in the store, as the planner wants them.
fn read_facts(conn: &Connection) -> StorageResult<Vec<VersionFacts>> {
    let mut statement = conn.prepare("SELECT id, note_id, at_ms, bytes FROM versions")?;
    let rows = statement.query_map([], |row| {
        Ok(VersionFacts {
            id: row.get(0)?,
            note: row.get(1)?,
            at: from_millis(row.get(2)?),
            bytes: row.get::<_, i64>(3)?.max(0) as u64,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Milliseconds since the epoch, which is how a time is written down here.
///
/// A time before the epoch is written as the epoch: no note's version was
/// captured in 1969, and a negative stamp would sort ahead of every real one.
fn as_millis(at: SystemTime) -> i64 {
    at.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| i64::try_from(since.as_millis()).ok())
        .unwrap_or(0)
}

/// The other direction.
fn from_millis(at_ms: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(at_ms.max(0) as u64)
}
