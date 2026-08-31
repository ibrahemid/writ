//! The path-keyed index over the notes folder (ADR-028 section 7).
//!
//! Files are the only copy of the text, so the index is keyed by the canonical
//! path of the file rather than by a database row. [`NotesIndex`] is the write
//! and query surface over `files` and `files_fts`; [`reconcile`] walks the
//! notes folder and brings the index back in line with what is on disk, which
//! is what makes deleting `writ.db` safe and what picks up notes written by
//! another editor while Writ was closed.
//!
//! # One key policy
//!
//! Three writers put rows into `files`: [`reconcile`], the notes watcher's
//! subscriber, and the deferred reindex behind a save. They must agree on the
//! spelling of a path byte for byte or the same note lands in the index twice
//! and a search returns a row nothing can open. [`index_key`] is that policy,
//! and every writer goes through it.
//!
//! # Files that are not downloaded
//!
//! A sync provider can leave a placeholder with no local data behind it.
//! Reading one asks the provider daemon to materialise it, which stalls the
//! walk and can pull gigabytes down a metered connection, so [`is_dataless`]
//! answers from the file's metadata and the walk skips those files without
//! opening them.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{params, Connection};
use writ_core::file_ops::{classify_path, FileOpenMode, THRESHOLD_NORMAL_BYTES};
use writ_core::search::{build_hit, SearchHit};
use writ_core::workspace::file_search::{rank_file_hits, FileHit};

use crate::errors::StorageResult;
use crate::workspace_search::build_walk;

/// Extensions indexed without sniffing the file, so the common case never
/// opens a file to decide whether to open it.
const TEXT_EXTENSIONS: &[&str] = &["md", "markdown", "txt", "text"];

/// macOS `SF_DATALESS`: the file has no local data behind it.
const SF_DATALESS: u32 = 0x4000_0000;

/// One indexed note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedNote {
    /// Canonical path, as produced by [`index_key`].
    pub path: String,
    /// File name, the label a result carries.
    pub name: String,
    /// Size in bytes at index time.
    pub size: u64,
    /// Modification time in milliseconds since the Unix epoch at index time.
    pub mtime: i64,
    /// Content hash, when the caller has one to record.
    pub hash: Option<String>,
}

impl IndexedNote {
    /// Describes the file at `path` as the index holds it, or `None` when its
    /// metadata cannot be read or it has no file name.
    ///
    /// The one place an [`IndexedNote`] is built from a path, so the reconcile
    /// walk and the save path key and label a file identically.
    pub fn from_file(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            path: index_key(path),
            name: path.file_name()?.to_string_lossy().into_owned(),
            size: metadata.len(),
            mtime: mtime_millis(&metadata),
            hash: None,
        })
    }
}

/// What one [`reconcile`] pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileOutcome {
    /// Files indexed for the first time.
    pub added: usize,
    /// Files re-read because their size or mtime had changed.
    pub updated: usize,
    /// Rows removed because the file behind them is gone.
    pub removed: usize,
    /// Files skipped because the filesystem reports them as not downloaded.
    pub skipped_dataless: usize,
    /// `true` when the walk stopped early because the caller cancelled it. A
    /// cancelled pass never removes rows: it has not seen the whole tree, so
    /// every unseen path is unproven rather than vanished.
    pub cancelled: bool,
}

/// The index's spelling of `path`.
///
/// Canonical, so `/var` and `/private/var` are one key on macOS, a symlinked
/// notes folder agrees with the walk that resolved it, and the filesystem's own
/// unicode normalisation is on both sides of every comparison. Case is
/// preserved: the key is also the path a result is opened by.
///
/// A path whose file is already gone cannot be canonicalised, so the parent is
/// canonicalised instead and the file name rejoined. That is the delete arm of
/// the watcher, and it must produce the same key the file had while it existed.
/// When nothing resolves, the path is returned as it came in, which keys the
/// row consistently even if it keys it by a spelling no walk will match.
pub fn index_key(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path)
        .ok()
        .or_else(|| {
            let parent = path.parent()?;
            let name = path.file_name()?;
            Some(std::fs::canonicalize(parent).ok()?.join(name))
        })
        .unwrap_or_else(|| path.to_path_buf());

    let text = resolved.to_string_lossy().into_owned();
    // Windows canonicalisation yields a `\\?\` verbatim prefix that no other
    // path in the app carries, matching `canonicalize_for_authorization`.
    match text.strip_prefix(r"\\?\") {
        Some(stripped) => stripped.to_string(),
        None => text,
    }
}

/// `true` when the filesystem reports the file has no local data: an iCloud (or
/// other provider) placeholder that reading would materialise.
///
/// macOS reads `SF_DATALESS` out of the stat flags, which is metadata only and
/// never opens the file. Every other platform answers `false`: no other
/// filesystem Writ supports reports the state, and guessing would skip real
/// files.
pub fn is_dataless(path: &Path) -> bool {
    stat_flags(path).is_some_and(dataless_from_flags)
}

/// The flag test, split out from the platform-specific stat so it is exercised
/// by the test suite on every platform rather than only on the one that can
/// produce the flag.
fn dataless_from_flags(flags: u32) -> bool {
    flags & SF_DATALESS != 0
}

#[cfg(target_os = "macos")]
fn stat_flags(path: &Path) -> Option<u32> {
    use std::os::macos::fs::MetadataExt;
    // symlink_metadata, so a link to a placeholder is judged by the link.
    std::fs::symlink_metadata(path).ok().map(|m| m.st_flags())
}

#[cfg(not(target_os = "macos"))]
fn stat_flags(_path: &Path) -> Option<u32> {
    None
}

/// Read and write access to the notes index over a borrowed connection.
pub struct NotesIndex<'a> {
    conn: &'a Connection,
}

impl<'a> NotesIndex<'a> {
    /// Constructs an index view over the given connection.
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Records `note` and its text, replacing whatever the index held for that
    /// path.
    ///
    /// The `files` row is updated in place rather than replaced. `files_fts`
    /// joins `files` on rowid and `links`, `properties`, `tags` and `headings`
    /// cascade from `files(path)`, so an `INSERT OR REPLACE` would reassign the
    /// rowid, orphan the text entry and delete every derived row a save is not
    /// entitled to touch (migration 040 records the same constraint).
    pub fn upsert(&self, note: &IndexedNote, content: &str) -> StorageResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO files (path, size, mtime, hash, indexed_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now'))
             ON CONFLICT(path) DO UPDATE SET
                 size = excluded.size,
                 mtime = excluded.mtime,
                 hash = excluded.hash,
                 indexed_at = excluded.indexed_at",
            params![note.path, note.size as i64, note.mtime, note.hash],
        )?;
        let rowid: i64 = tx.query_row(
            "SELECT rowid FROM files WHERE path = ?1",
            params![note.path],
            |row| row.get(0),
        )?;
        tx.execute("DELETE FROM files_fts WHERE rowid = ?1", params![rowid])?;
        tx.execute(
            "INSERT INTO files_fts (rowid, name, content) VALUES (?1, ?2, ?3)",
            params![rowid, note.name, content],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// A walk's write, which loses every race it is in.
    ///
    /// A walk reads a file and writes the row some milliseconds later. A save
    /// landing in that window would be overwritten by what the walk read
    /// before it, and nothing would correct the row until the file changed
    /// again. So the write is conditional on the row still holding `expected`
    /// — the state the walk decided from, or `None` for a row it did not see —
    /// and a walk that finds anything else leaves the newer entry alone.
    /// `false` says the write was declined.
    fn upsert_walked(
        &self,
        note: &IndexedNote,
        content: &str,
        expected: Option<(u64, i64)>,
    ) -> StorageResult<bool> {
        let tx = self.conn.unchecked_transaction()?;
        let wrote = match expected {
            None => tx.execute(
                "INSERT INTO files (path, size, mtime, hash, indexed_at)
                 VALUES (?1, ?2, ?3, ?4, datetime('now'))
                 ON CONFLICT(path) DO NOTHING",
                params![note.path, note.size as i64, note.mtime, note.hash],
            )?,
            Some((size, mtime)) => tx.execute(
                "UPDATE files
                    SET size = ?2, mtime = ?3, hash = ?4, indexed_at = datetime('now')
                  WHERE path = ?1 AND size = ?5 AND mtime = ?6",
                params![
                    note.path,
                    note.size as i64,
                    note.mtime,
                    note.hash,
                    size as i64,
                    mtime
                ],
            )?,
        };
        if wrote == 0 {
            return Ok(false);
        }
        let rowid: i64 = tx.query_row(
            "SELECT rowid FROM files WHERE path = ?1",
            params![note.path],
            |row| row.get(0),
        )?;
        tx.execute("DELETE FROM files_fts WHERE rowid = ?1", params![rowid])?;
        tx.execute(
            "INSERT INTO files_fts (rowid, name, content) VALUES (?1, ?2, ?3)",
            params![rowid, note.name, content],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Removes `path` from the index.
    ///
    /// The text entry goes first, while the `files` row it joins on still
    /// exists; the row itself follows, and `links`, `properties`, `tags` and
    /// `headings` cascade with it. Losing the row without losing the text entry
    /// is what produces a hit nothing can open, so both steps propagate errors.
    pub fn remove(&self, path: &str) -> StorageResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM files_fts
             WHERE rowid = (SELECT rowid FROM files WHERE path = ?1)",
            params![path],
        )?;
        tx.execute("DELETE FROM files WHERE path = ?1", params![path])?;
        tx.commit()?;
        Ok(())
    }

    /// Re-keys every row under `from` to the same file under `to`.
    ///
    /// The move already put the files there, so the rows describe the right
    /// bytes at the wrong key. Rewriting the key rather than dropping the rows
    /// is what keeps the following walk from reading anything: `size` and
    /// `mtime` still match the file, so `reconcile` reports no work. Dropping
    /// them would make every note in the folder look new, and in a sync folder
    /// a walk that reads every note pulls down every note (ADR-028 section 7).
    ///
    /// `files.path` is the parent of `links`, `properties`, `tags` and
    /// `headings`, which cascade on delete but have no `ON UPDATE`, so the
    /// children are rewritten in the same transaction with the constraint
    /// deferred to its commit. `files_fts` joins on rowid, which an `UPDATE`
    /// leaves alone, so the text entries need no work at all.
    ///
    /// Both folders are spelled through [`index_key`], so a caller may pass
    /// either folder's everyday path: `from` no longer exists by the time this
    /// runs, and its key comes from its parent exactly as a deleted file's
    /// does. Prefix comparison is against the folder plus a separator, so a
    /// move out of `~/Writ` never claims a row in `~/Writing`. Returns the
    /// rows re-keyed.
    pub fn rekey_root(&self, from: &Path, to: &Path) -> StorageResult<usize> {
        let old = root_prefix(from);
        let new = root_prefix(to);
        if old == new {
            return Ok(0);
        }
        let width = old.chars().count() as i64;
        let rest = width + 1;

        let tx = self.conn.unchecked_transaction()?;
        // Scoped to this transaction and reset at its commit: the children
        // point at a parent key that does not exist yet for the length of the
        // rewrite, which an immediate constraint would refuse.
        tx.pragma_update(None, "defer_foreign_keys", "on")?;

        let changed = tx.execute(
            "UPDATE files SET path = ?1 || substr(path, ?2) WHERE substr(path, 1, ?3) = ?4",
            params![new, rest, width, old],
        )?;
        for (table, column) in [
            ("links", "from_path"),
            ("links", "to_path"),
            ("properties", "path"),
            ("tags", "path"),
            ("headings", "path"),
        ] {
            tx.execute(
                &format!(
                    "UPDATE {table} SET {column} = ?1 || substr({column}, ?2) \
                     WHERE substr({column}, 1, ?3) = ?4"
                ),
                params![new, rest, width, old],
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Up to `limit` ranked content hits, each carrying the note's path, name,
    /// matching line and a highlighted snippet.
    pub fn search_hits(
        &self,
        query: &str,
        terms: &[String],
        limit: usize,
    ) -> StorageResult<Vec<SearchHit>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.path, x.name, x.content FROM files_fts x
             JOIN files f ON f.rowid = x.rowid
             WHERE files_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        let mut hits = Vec::new();
        for row in rows {
            let (path, name, content) = row?;
            // A note has no buffer id until something opens it. The adapter
            // fills one in for a note that is already a tab; the path is what
            // opens the rest.
            let mut hit = build_hit("", &name, &content, terms);
            hit.path = Some(path);
            hits.push(hit);
        }
        Ok(hits)
    }

    /// Total number of notes matching `query`, independent of any result limit,
    /// so the UI can report "N of M" honestly.
    pub fn count(&self, query: &str) -> StorageResult<usize> {
        let total: i64 = self.conn.query_row(
            "SELECT count(*) FROM files_fts WHERE files_fts MATCH ?1",
            params![query],
            |row| row.get(0),
        )?;
        Ok(total as usize)
    }

    /// Up to `limit` ranked name-only hits, for quick open.
    ///
    /// Ranked by the same subsequence scorer the workspace file palette uses,
    /// so a prefix of a note's name outranks a match in the middle of one and
    /// the two surfaces order their results the same way.
    pub fn search_names(&self, query: &str, limit: usize) -> StorageResult<Vec<FileHit>> {
        let mut stmt = self
            .conn
            .prepare("SELECT f.path, x.name FROM files f JOIN files_fts x ON x.rowid = f.rowid")?;
        let candidates = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rank_file_hits(
            query,
            candidates
                .iter()
                .map(|(path, name)| (path.as_str(), name.as_str())),
            limit,
        ))
    }

    /// Every indexed path with the size and mtime it was indexed at, for
    /// reconciliation.
    pub fn snapshot(&self) -> StorageResult<Vec<(String, u64, i64)>> {
        let mut stmt = self.conn.prepare("SELECT path, size, mtime FROM files")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

/// Walks `notes_root` and brings the index in line with it: new files are
/// added, files whose size or mtime moved are re-read, and rows whose file is
/// gone are removed.
///
/// The walk is the shared search walk ([`build_walk`]), so the notes index, the
/// name index and the content grep agree on which files exist and which folders
/// another client left behind (`.obsidian`, `.trash`, `.stfolder`,
/// `.stversions`) are skipped.
///
/// `cancelled` is polled per entry so a shutdown does not wait for a large
/// folder. A cancelled pass reports what it did and removes nothing: it has not
/// seen the whole tree, so an unseen path is unproven rather than vanished.
///
/// `is_dataless` is injected rather than called directly so the skip is
/// testable without a real sync placeholder; production passes [`is_dataless`].
pub fn reconcile(
    conn: &Connection,
    notes_root: &Path,
    cancelled: &dyn Fn() -> bool,
    is_dataless: &dyn Fn(&Path) -> bool,
) -> StorageResult<ReconcileOutcome> {
    let index = NotesIndex::new(conn);
    let known: HashMap<String, (u64, i64)> = index
        .snapshot()?
        .into_iter()
        .map(|(path, size, mtime)| (path, (size, mtime)))
        .collect();

    let mut outcome = ReconcileOutcome::default();
    let mut seen: Vec<String> = Vec::with_capacity(known.len());

    for entry in build_walk(notes_root).build() {
        if cancelled() {
            outcome.cancelled = true;
            break;
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();

        // Before anything that opens the file: a placeholder is materialised by
        // the read, not by the decision to read.
        if is_dataless(path) {
            outcome.skipped_dataless += 1;
            continue;
        }
        let Ok(metadata) = std::fs::metadata(path) else {
            continue;
        };
        let size = metadata.len();

        // The same ceiling `BufferStore::index_note` applies. Indexing a file
        // the save path will not re-index leaves its first contents in the
        // index for good.
        if size > THRESHOLD_NORMAL_BYTES {
            continue;
        }
        if !should_index(path) {
            continue;
        }

        let mtime = mtime_millis(&metadata);
        let key = index_key(path);

        let expected = match known.get(&key) {
            Some(&state) if state == (size, mtime) => {
                seen.push(key);
                continue;
            }
            other => other.copied(),
        };

        let Ok(content) = std::fs::read_to_string(path) else {
            // Not text after all (or unreadable): leave it out rather than
            // index bytes nobody can search.
            seen.push(key);
            continue;
        };

        let name = entry.file_name().to_string_lossy().into_owned();
        seen.push(key.clone());
        let note = IndexedNote {
            path: key,
            name,
            size,
            mtime,
            hash: None,
        };

        // One transaction per file. The walk runs on its own connection, so a
        // long-held write lock would stall saves for the length of the walk.
        if index.upsert_walked(&note, &content, expected)? {
            if expected.is_some() {
                outcome.updated += 1;
            } else {
                outcome.added += 1;
            }
        }
    }

    if !outcome.cancelled {
        let seen: std::collections::HashSet<String> = seen.into_iter().collect();
        for path in known.keys() {
            if seen.contains(path) {
                continue;
            }
            index.remove(path)?;
            outcome.removed += 1;
        }
    }

    Ok(outcome)
}

/// A folder's index key with a trailing separator, the form a prefix
/// comparison needs so a folder never claims a sibling whose name it is a
/// prefix of.
fn root_prefix(path: &Path) -> String {
    let mut text = index_key(path);
    if !text.ends_with(std::path::MAIN_SEPARATOR) {
        text.push(std::path::MAIN_SEPARATOR);
    }
    text
}

/// Whether `path` holds note text worth indexing.
///
/// A known text extension is taken at its word so the common case never opens
/// the file, which means this answers the *kind* question only: callers gate
/// on size themselves. Anything without a known extension is classified, and
/// only a file that opens with the full feature set is indexed.
fn should_index(path: &Path) -> bool {
    // The name `write_atomic` gives the file it writes before renaming it into
    // place. A walk that runs while a note is being saved would otherwise index
    // the half-written copy, and leave a row for a file that no longer exists.
    // Same test the notes watcher applies to an event.
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    if name.starts_with(".tmp") || name.ends_with(".tmp") {
        return false;
    }

    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if TEXT_EXTENSIONS.iter().any(|t| t.eq_ignore_ascii_case(ext)) {
            return true;
        }
    }
    matches!(
        classify_path(path).map(|c| c.mode),
        Ok(FileOpenMode::Normal)
    )
}

/// Modification time in milliseconds since the Unix epoch, or `0` when the
/// filesystem does not report one. Milliseconds rather than seconds so two
/// edits inside one second are still two different index states.
fn mtime_millis(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The notes index over a connection of its own.
///
/// The adapter's [`crate::buffer_store::BufferStore`] holds a single connection
/// behind a mutex that every save and every tab operation queues on. A
/// reconcile of a large notes folder would hold it for the whole walk, and a
/// keystroke would wait for it, so the index that the walk and the search box
/// use opens its own connection to the same database. WAL lets the two proceed
/// together: readers do not block the writer, and the walk commits one file at
/// a time so the write lock is never held for long.
pub struct NotesIndexStore {
    conn: Mutex<Connection>,
    generation: AtomicU64,
}

impl NotesIndexStore {
    /// Opens a connection of its own to the database at `db_path`.
    ///
    /// Migrations are not run here. The primary connection runs them inside
    /// `AppState::initialize` before this one is opened, which is the same
    /// ordering the layout store relies on.
    pub fn open(db_path: &Path) -> StorageResult<Self> {
        Ok(Self {
            conn: Mutex::new(crate::database::connection::open_database(db_path)?),
            generation: AtomicU64::new(0),
        })
    }

    /// Which folder the index is describing, as a number that changes whenever
    /// it changes.
    ///
    /// A walk takes seconds. If the notes folder moves while one is running,
    /// that walk finishes against the old folder and prunes every row it did
    /// not see, which is every row the move re-keyed. A walk therefore captures
    /// this value when it starts and gives up as soon as it differs.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Retires every walk in flight. Called before the rows move, so no walk
    /// started against the old folder can still be running when they do.
    pub fn bump_generation(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Re-keys the index from one notes folder to another. See
    /// [`NotesIndex::rekey_root`].
    pub fn rekey_root(&self, from: &Path, to: &Path) -> StorageResult<usize> {
        NotesIndex::new(&self.conn()).rekey_root(from, to)
    }

    /// A poisoned index lock is recovered rather than cascaded: the index is
    /// derived data that a reconcile rebuilds, so a panic while holding it
    /// costs at most a stale row, and refusing every later search would cost
    /// the user the feature.
    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Up to `limit` ranked content hits. See [`NotesIndex::search_hits`].
    pub fn search_hits(
        &self,
        query: &str,
        terms: &[String],
        limit: usize,
    ) -> StorageResult<Vec<SearchHit>> {
        NotesIndex::new(&self.conn()).search_hits(query, terms, limit)
    }

    /// Total number of notes matching `query`. See [`NotesIndex::count`].
    pub fn count(&self, query: &str) -> StorageResult<usize> {
        NotesIndex::new(&self.conn()).count(query)
    }

    /// Up to `limit` ranked name hits. See [`NotesIndex::search_names`].
    pub fn search_names(&self, query: &str, limit: usize) -> StorageResult<Vec<FileHit>> {
        NotesIndex::new(&self.conn()).search_names(query, limit)
    }

    /// Records the file at `path` in the index, reading its text.
    ///
    /// The watcher's create-or-modify arm. A file the filesystem reports as not
    /// downloaded, one over [`THRESHOLD_NORMAL_BYTES`], or one that is not note
    /// text is left alone rather than materialised or indexed as bytes; `false`
    /// says nothing was recorded.
    pub fn index_path(&self, path: &Path) -> StorageResult<bool> {
        if is_dataless(path) {
            return Ok(false);
        }
        let Some(note) = IndexedNote::from_file(path) else {
            return Ok(false);
        };
        if note.size > THRESHOLD_NORMAL_BYTES || !should_index(path) {
            return Ok(false);
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            return Ok(false);
        };
        NotesIndex::new(&self.conn()).upsert(&note, &content)?;
        Ok(true)
    }

    /// Removes the file at `path` from the index.
    ///
    /// The watcher's delete arm, and the second half of a rename. Removing a
    /// path the index does not hold is not an error, which is what lets the
    /// subscriber replay an event without checking first.
    pub fn forget_path(&self, path: &Path) -> StorageResult<()> {
        NotesIndex::new(&self.conn()).remove(&index_key(path))
    }

    /// Walks `notes_root` and brings the index in line with it. See
    /// [`reconcile`].
    pub fn reconcile(
        &self,
        notes_root: &Path,
        cancelled: &dyn Fn() -> bool,
        is_dataless: &dyn Fn(&Path) -> bool,
    ) -> StorageResult<ReconcileOutcome> {
        reconcile(&self.conn(), notes_root, cancelled, is_dataless)
    }

    /// Every indexed path with its size and mtime. See [`NotesIndex::snapshot`].
    pub fn snapshot(&self) -> StorageResult<Vec<(String, u64, i64)>> {
        NotesIndex::new(&self.conn()).snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataless_from_flags_reads_the_sf_dataless_bit() {
        assert!(dataless_from_flags(SF_DATALESS));
        assert!(dataless_from_flags(SF_DATALESS | 0x0000_0002));
    }

    #[test]
    fn dataless_from_flags_ignores_every_other_flag() {
        assert!(!dataless_from_flags(0));
        assert!(!dataless_from_flags(0x0000_0002));
        assert!(!dataless_from_flags(0x0080_0000));
    }

    #[test]
    fn a_file_with_local_data_is_not_dataless_on_any_platform() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("here.md");
        std::fs::write(&path, "local").expect("write");
        assert!(!is_dataless(&path));
    }

    #[test]
    fn index_key_of_a_missing_file_matches_the_key_it_had() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("gone.md");
        std::fs::write(&path, "body").expect("write");
        let before = index_key(&path);
        std::fs::remove_file(&path).expect("remove");
        assert_eq!(before, index_key(&path));
    }

    #[test]
    fn should_index_accepts_the_text_extensions() {
        for name in ["a.md", "a.markdown", "a.txt", "a.text", "a.MD"] {
            assert!(
                should_index(Path::new(name)),
                "{name} must be indexed without being opened"
            );
        }
    }
}
