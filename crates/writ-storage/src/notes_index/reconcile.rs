//! The walk that brings the index in line with the notes folder.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use rusqlite::{params, Connection};
use writ_core::file_ops::THRESHOLD_NORMAL_BYTES;
use writ_core::notes::{facts, links};
use writ_core::workspace::path_has_notes_ignored_name;

use super::resolve::{stored_path, NameIndex, NameSource};
use super::upsert::{self, Expect};
use super::walk::{build_notes_walk, has_text_extension, mtime_millis, should_index};
use super::{index_key, schema, IndexedBy, IndexedNote};
use crate::errors::StorageResult;

/// Link rows the closing pass reads and writes at a time. Small enough that a
/// save waiting on the writer waits for one batch, never for the whole pass.
const RESOLVE_BATCH: i64 = 2_000;

/// What one [`reconcile`] pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileOutcome {
    /// Files indexed for the first time.
    pub added: usize,
    /// Files re-read because their size or mtime had changed.
    pub updated: usize,
    /// Rows removed because the file behind them is gone.
    pub removed: usize,
    /// Files the filesystem reports as not downloaded, which are indexed by
    /// name with no text rather than read.
    pub skipped_dataless: usize,
    /// `true` when the walk stopped early because the caller cancelled it. A
    /// cancelled pass never removes rows: it has not seen the whole tree, so
    /// every unseen path is unproven rather than vanished.
    pub cancelled: bool,
}

/// The writes made through the store while a walk is under way.
///
/// A walk reads a file with nothing held and writes its rows a moment later,
/// and it removes the rows of the files it did not find only once it has been
/// through the whole folder. A save, the notes watcher or a rename can write
/// the same path in between, and what they write is newer than what the walk
/// saw: a note that was away when the walk reached it and came back before the
/// walk ended is indexed by the write that brought it back, and a removal at
/// the walk's end would delete it again although the file is there. Every such
/// write is recorded here, and the walk leaves alone a path written after it
/// looked (ADR-036 section 7).
///
/// Nothing is recorded while no walk is under way, and the record is dropped
/// when the last one ends.
#[derive(Debug, Default)]
pub(crate) struct RecentWrites {
    /// Walks under way.
    walks: usize,
    /// Counts the writes recorded, so a walk can tell which came after a point
    /// it took.
    clock: u64,
    /// The clock at each path's latest write.
    written: HashMap<String, u64>,
    /// The clock at the latest write that changed every path at once: the
    /// notes folder moved and every row was re-keyed.
    every_path: u64,
}

impl RecentWrites {
    /// Records that `path`'s rows were written.
    pub(crate) fn wrote(&mut self, path: &str) {
        if self.walks == 0 {
            return;
        }
        self.clock += 1;
        self.written.insert(path.to_owned(), self.clock);
    }

    /// Records that every row changed its path at once.
    pub(crate) fn moved_every_path(&mut self) {
        if self.walks == 0 {
            return;
        }
        self.clock += 1;
        self.every_path = self.clock;
    }

    fn walk_started(&mut self) -> u64 {
        self.walks += 1;
        self.clock
    }

    fn walk_ended(&mut self) {
        self.walks = self.walks.saturating_sub(1);
        if self.walks == 0 {
            self.written.clear();
        }
    }

    fn written_since(&self, path: &str, point: u64) -> bool {
        self.every_path > point || self.written.get(path).is_some_and(|&at| at > point)
    }
}

/// One walk's place in [`RecentWrites`], from the moment it starts until it
/// returns, however it returns.
struct Walking<'a> {
    writes: &'a Mutex<RecentWrites>,
    /// The point the walk started at, before it read what the index held.
    started: u64,
}

impl<'a> Walking<'a> {
    fn start(writes: &'a Mutex<RecentWrites>) -> Self {
        let started = lock(writes).walk_started();
        Self { writes, started }
    }

    /// The point anything the walk learns from here on dates from.
    fn now(&self) -> u64 {
        lock(self.writes).clock
    }

    /// Whether `path` was written through the store after `point`. Asked while
    /// the writer is held, so no write can land between the answer and the
    /// write it decides.
    fn overtaken(&self, path: &str, point: u64) -> bool {
        lock(self.writes).written_since(path, point)
    }
}

impl Drop for Walking<'_> {
    fn drop(&mut self) {
        lock(self.writes).walk_ended();
    }
}

/// A poisoned lock is recovered rather than cascaded: what it guards is a
/// record of writes, and a panic while holding it costs at most a removal the
/// walk leaves for the next one.
fn lock(writes: &Mutex<RecentWrites>) -> MutexGuard<'_, RecentWrites> {
    writes.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How a walk reaches the index: where it reads, where it writes, and the
/// names it resolves against.
///
/// Over one borrowed connection, reads and writes share it. The app's
/// [`super::NotesIndexStore`] reads through a connection of its own and
/// holds its writer for one write at a time, which is what lets every other
/// command run between two files of a walk.
pub(crate) trait IndexAccess {
    /// Where the names live.
    type Names: NameSource;
    /// A read of what the index holds now.
    fn read<R>(&self, f: impl FnOnce(&Connection) -> StorageResult<R>) -> StorageResult<R>;
    /// One of the walk's writes: the writer connection and the names, for `f`
    /// and no longer. The app's store lets every other write waiting for the
    /// writer go first.
    fn write<R>(
        &self,
        f: impl FnOnce(&Connection, &Self::Names) -> StorageResult<R>,
    ) -> StorageResult<R>;
    /// A look at the names, holding nothing a write needs.
    fn with_names<R>(&self, f: impl FnOnce(&NameIndex) -> R) -> StorageResult<R>;
    /// The writes made beside the walk, which it leaves alone.
    fn recent_writes(&self) -> &Mutex<RecentWrites>;
}

/// One borrowed connection that reads and writes, and names of its own.
///
/// Nothing writes through it but the walk, so its record of other writes
/// stays empty.
struct OneConnection<'a> {
    conn: &'a Connection,
    names: RefCell<NameIndex>,
    recent_writes: Mutex<RecentWrites>,
}

impl IndexAccess for OneConnection<'_> {
    type Names = RefCell<NameIndex>;

    fn read<R>(&self, f: impl FnOnce(&Connection) -> StorageResult<R>) -> StorageResult<R> {
        f(self.conn)
    }

    fn write<R>(
        &self,
        f: impl FnOnce(&Connection, &Self::Names) -> StorageResult<R>,
    ) -> StorageResult<R> {
        f(self.conn, &self.names)
    }

    fn with_names<R>(&self, f: impl FnOnce(&NameIndex) -> R) -> StorageResult<R> {
        Ok(f(&self.names.borrow()))
    }

    fn recent_writes(&self) -> &Mutex<RecentWrites> {
        &self.recent_writes
    }
}

/// Walks `notes_root` and brings the index on `conn` in line with it: new
/// files are added, files whose size or mtime moved are re-read, and rows
/// whose file is gone are removed.
///
/// The walk is the notes walk ([`build_notes_walk`]): every file in the
/// folder, folders called `vendor`, `build` or `coverage` included, no
/// `.gitignore` consulted, and only `.git`, what another client or Finder
/// keeps (`.obsidian`, `.trash`, `.stfolder`, `.stversions`, `.DS_Store`) and
/// temp-file names left out (ADR-028 section 7).
///
/// `cancelled` is polled per entry, and once more before anything is removed,
/// so a shutdown does not wait for a large folder. A cancelled pass reports
/// what it did and removes nothing: it has not seen the whole tree, so an
/// unseen path is unproven rather than vanished.
///
/// `is_dataless` is injected rather than called directly so the skip is
/// testable without a real sync placeholder; production passes
/// [`super::is_dataless`].
///
/// # Rebuilding the derived tables
///
/// `links`, `properties`, `tags` and `headings` are derived: emptying them and
/// running this rebuilds them (ADR-034). The size-and-mtime shortcut is what
/// would stop that — every file still matches its row, so nothing would be
/// re-read — so a pass that finds fewer derived rows than the last complete
/// pass left behind, over a file count that has not shrunk, re-reads every
/// file. The census is kept in `schema_meta`, so an index written before the
/// four tables were filled at all rebuilds on its first pass. So does an index
/// filled under another [`facts::READING`], recorded beside the census: the
/// rows are re-derived once, then left alone again.
///
/// # Links
///
/// Each file's links are resolved as it is written, and a note the walk adds
/// fills in the links that were waiting for it in the same write. A note the
/// walk removes empties the links that reached it and hands them to any other
/// note of the same name. A complete pass ends by checking every stored link
/// against the folder's names once more, a batch at a time, which is what
/// brings back in line a row written some other way.
///
/// # Writes beside the walk
///
/// [`super::NotesIndexStore`] holds its writer for one file at a time, so a
/// save, the notes watcher and a rename write between two files of a walk,
/// and what they write is newer than what the walk saw. The walk leaves alone
/// a path the store wrote after the walk looked at it ([`RecentWrites`]): it
/// does not write that file's rows, and it does not remove them at its end. A
/// folder re-keyed under it counts as every path written.
pub fn reconcile(
    conn: &Connection,
    notes_root: &Path,
    cancelled: &dyn Fn() -> bool,
    is_dataless: &dyn Fn(&Path) -> bool,
) -> StorageResult<ReconcileOutcome> {
    let access = OneConnection {
        conn,
        names: RefCell::new(NameIndex::load(conn)?),
        recent_writes: Mutex::default(),
    };
    walk(&access, notes_root, cancelled, is_dataless)
}

/// [`reconcile`] through any [`IndexAccess`].
pub(crate) fn walk<A: IndexAccess>(
    access: &A,
    notes_root: &Path,
    cancelled: &dyn Fn() -> bool,
    is_dataless: &dyn Fn(&Path) -> bool,
) -> StorageResult<ReconcileOutcome> {
    // Before the index is read, so a write that lands after the read is one
    // this walk knows about.
    let walking = Walking::start(access.recent_writes());
    let (known, name_only, rebuild_facts) = access.read(|conn| {
        let known: HashMap<String, (u64, i64)> = schema::snapshot(conn)?
            .into_iter()
            .map(|(path, size, mtime)| (path, (size, mtime)))
            .collect();
        let name_only = schema::name_only_paths(conn)?;
        let census = schema::facts_census(conn)?;
        // Rows derived under another reading of a note match their files'
        // size and mtime exactly, so nothing else would touch them. They are
        // not wrong for the files; they are wrong for what a note now means.
        let stale_reading = schema::read_facts_reading(conn)? != Some(facts::READING);
        let rebuild_facts = stale_reading
            || match schema::read_facts_census(conn)? {
                // Nothing recorded: either the index is empty, or it was
                // written before this pass knew how to derive anything, and
                // those rows need one read.
                None => census.1 > 0,
                // Fewer derived rows than the last complete pass left, over at
                // least as many files. Notes deleted outside Writ take their
                // rows with them and shrink both numbers, which is not this.
                Some((rows, files)) => census.0 < rows && census.1 >= files,
            };
        Ok((known, name_only, rebuild_facts))
    })?;

    let mut outcome = ReconcileOutcome::default();
    let mut seen: HashSet<String> = HashSet::with_capacity(known.len());

    for entry in build_notes_walk(notes_root).build() {
        if cancelled() {
            outcome.cancelled = true;
            break;
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();

        // A name no listing, index or watcher event carries, on the file or on
        // a folder above it. Decided from the path alone, before anything
        // touches the file.
        if path_has_notes_ignored_name(notes_root, path) {
            continue;
        }

        // Everything the walk learns about this file dates from here.
        let looked_at = walking.now();

        // Before anything that opens the file: a placeholder is materialised by
        // the read, not by the decision to read.
        let dataless = is_dataless(path);
        if dataless {
            outcome.skipped_dataless += 1;
            // The kind question is settled by the extension or not at all:
            // `should_index`'s fallback sniffs the bytes, which is the read
            // this whole branch exists to avoid.
            if !has_text_extension(path) {
                continue;
            }
        }

        let Ok(metadata) = std::fs::metadata(path) else {
            continue;
        };
        let size = metadata.len();

        if !dataless {
            // The same ceiling `BufferStore::index_note` applies. Indexing a
            // file the save path will not re-index leaves its first contents in
            // the index for good. The ceiling bounds a read, so a placeholder,
            // which is never read, is not measured against it: an evicted
            // 20 MB note keeps its name in the index.
            if size > THRESHOLD_NORMAL_BYTES {
                continue;
            }
            if !should_index(path) {
                continue;
            }
        }

        let mtime = mtime_millis(&metadata);
        let key = index_key(path);

        // A download leaves size and mtime alone, so an unchanged file whose
        // row holds nothing but its name is read now: this is the pass where a
        // placeholder became a note.
        let downloaded = !dataless && name_only.contains(&key);

        let expect = match known.get(&key) {
            Some(&state) if state == (size, mtime) && !downloaded && !rebuild_facts => {
                seen.insert(key);
                continue;
            }
            Some(&(size, mtime)) => Expect::Row(size, mtime),
            None => Expect::NoRow,
        };

        // A placeholder is indexed by name and nothing else: the row and the
        // name entry are what make it findable and openable, and the empty
        // text is the honest record of what has been read (ADR-028 section 7).
        let content = if dataless {
            String::new()
        } else {
            let Ok(content) = std::fs::read_to_string(path) else {
                // Not text after all (or unreadable): leave it out rather than
                // index bytes nobody can search, and let any row it already had
                // be pruned. A row kept here would answer searches with text the
                // file no longer holds. Same answer the size gate above gives.
                continue;
            };
            content
        };

        let name = entry.file_name().to_string_lossy().into_owned();
        seen.insert(key.clone());
        let note = IndexedNote {
            path: key,
            name,
            size,
            mtime,
            // No read happened for a placeholder, so there is no digest to
            // record and the row says so in `indexed_by`.
            hash: None,
            indexed_by: if dataless {
                IndexedBy::Name
            } else {
                IndexedBy::Content
            },
        };

        // One transaction per file, and the writer held for that one file: the
        // read above happened before it was taken.
        let wrote = record(access, &walking, looked_at, &note, &content, expect)?;
        if wrote {
            match expect {
                Expect::NoRow => outcome.added += 1,
                Expect::Row(..) | Expect::Anything => outcome.updated += 1,
            }
        }
    }

    // A cancelled pass stops here. What it wrote is resolved already, file by
    // file, and the closing check below is for a pass that saw the whole
    // folder: a shutdown or a folder move is not kept waiting for it. Asked
    // once more because a walk can run out of entries after it was told to
    // stop: a folder moved away from under it lists nothing further.
    if outcome.cancelled || cancelled() {
        outcome.cancelled = true;
        return Ok(outcome);
    }

    for path in known.keys() {
        if seen.contains(path) {
            continue;
        }
        if remove(access, &walking, path)? {
            outcome.removed += 1;
        }
    }

    resolve_every_link(access)?;

    // Only a complete pass may speak for the whole folder. Taken on the writer,
    // so no save lands between the count and the record of it.
    access.write(|conn, _| schema::record_complete_pass(conn))?;

    Ok(outcome)
}

/// Writes what the walk read of one file, unless the store wrote that path
/// after `looked_at`: whatever wrote it then knew more than the walk does.
/// `false` says nothing was written.
///
/// A note read, then deleted and forgotten before this runs, would otherwise
/// come back as a row for a file that is gone, which only the next walk
/// removes.
fn record<A: IndexAccess>(
    access: &A,
    walking: &Walking<'_>,
    looked_at: u64,
    note: &IndexedNote,
    content: &str,
    expect: Expect,
) -> StorageResult<bool> {
    access.write(|conn, names| {
        if walking.overtaken(&note.path, looked_at) {
            return Ok(false);
        }
        upsert::write_file(conn, names, note, content, expect)
    })
}

/// Removes the rows of `path`, a file the walk did not find, unless the store
/// wrote it after the walk started. `false` says the rows were left.
///
/// The walk cannot tell when it would have reached the file, so any write
/// since its start is taken as newer than its own look.
fn remove<A: IndexAccess>(access: &A, walking: &Walking<'_>, path: &str) -> StorageResult<bool> {
    access.write(|conn, names| {
        if walking.overtaken(path, walking.started) {
            return Ok(false);
        }
        upsert::forget_file(conn, names, path)?;
        Ok(true)
    })
}

/// Checks every stored link against the folder's names and points the ones
/// that disagree at the note they name now. Returns the links changed.
///
/// The links a walk writes are resolved as they are written, so this finds
/// nothing in the common case; it is what brings back in line a row some
/// other writer left. It reads a batch of rows at a time, resolves them
/// without holding the writer, and takes the writer only for a batch that has
/// something to change, re-resolving each row under it and writing it only if
/// the row still holds what was read. A save between the read and the write is
/// therefore never undone.
fn resolve_every_link<A: IndexAccess>(access: &A) -> StorageResult<usize> {
    let mut after = 0i64;
    let mut changed = 0usize;
    loop {
        let batch: Vec<(i64, String, String, Option<String>)> = access.read(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT rowid, from_path, to_target, to_path FROM links
                  WHERE rowid > ?1 ORDER BY rowid LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![after, RESOLVE_BATCH], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        let Some(last) = batch.last().map(|row| row.0) else {
            break;
        };
        after = last;

        let stale: Vec<(i64, String, String, Option<String>)> = access.with_names(|names| {
            batch
                .into_iter()
                .filter(|(_, from_path, to_target, was)| {
                    stored_path(names.resolve(&links::stored_target(to_target), from_path)) != *was
                })
                .collect()
        })?;
        if stale.is_empty() {
            continue;
        }

        changed += access.write(|conn, names| {
            let tx = upsert::begin_write(conn, names)?;
            let written = names.read(|names| -> StorageResult<usize> {
                let mut written = 0usize;
                for (rowid, from_path, to_target, was) in &stale {
                    let now =
                        stored_path(names.resolve(&links::stored_target(to_target), from_path));
                    if now == *was {
                        continue;
                    }
                    written += tx.execute(
                        "UPDATE links SET to_path = ?2
                          WHERE rowid = ?1 AND from_path = ?3 AND to_target = ?4
                            AND to_path IS ?5",
                        params![rowid, now, from_path, to_target, was],
                    )?;
                }
                Ok(written)
            })?;
            tx.commit()?;
            Ok(written)
        })?;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::{record, IndexAccess, Walking};
    use crate::database::connection::open_database;
    use crate::database::migrations::run_migrations;
    use crate::notes_index::upsert::Expect;
    use crate::notes_index::{IndexedNote, NotesIndexStore};

    fn store() -> (TempDir, NotesIndexStore, PathBuf) {
        let dir = TempDir::new().expect("tempdir");
        let db_path = dir.path().join("writ.db");
        run_migrations(&open_database(&db_path).expect("open_database")).expect("migrations");
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&notes).expect("notes folder");
        let index = NotesIndexStore::open(&db_path).expect("index");
        (dir, index, notes)
    }

    fn read(path: &Path) -> (IndexedNote, String) {
        let note = IndexedNote::from_file(path).expect("metadata");
        let content = std::fs::read_to_string(path).expect("read");
        (note, content)
    }

    fn indexed(index: &NotesIndexStore) -> Vec<String> {
        index
            .snapshot()
            .expect("snapshot")
            .into_iter()
            .map(|(path, ..)| path)
            .collect()
    }

    /// A walk reads a new note and writes its rows a moment later. A delete
    /// the watcher reports in between is newer than the read, so the walk
    /// writes nothing: the row would name a file that is gone.
    #[test]
    fn a_note_forgotten_after_the_walk_read_it_is_not_written_back() {
        let (_dir, index, notes) = store();
        let path = notes.join("Gone.md");
        std::fs::write(&path, "short-lived\n").expect("write");

        let walking = Walking::start(index.recent_writes());
        let looked_at = walking.now();
        let (note, content) = read(&path);
        std::fs::remove_file(&path).expect("delete");
        index.forget_path(&path).expect("forget");

        let wrote =
            record(&index, &walking, looked_at, &note, &content, Expect::NoRow).expect("record");

        assert!(!wrote);
        assert!(
            indexed(&index).is_empty(),
            "a row names a file that is gone"
        );
    }

    /// A save that lands after the walk read a note is the newer text, so the
    /// walk does not write what it read over it.
    #[test]
    fn a_note_saved_after_the_walk_read_it_keeps_the_saved_text() {
        let (_dir, index, notes) = store();
        let path = notes.join("Plan.md");
        std::fs::write(&path, "first draft\n").expect("write");

        let walking = Walking::start(index.recent_writes());
        let looked_at = walking.now();
        let (note, content) = read(&path);
        std::fs::write(&path, "second draft, kestrel\n").expect("save");
        assert!(index.index_path(&path).expect("save's reindex"));

        let wrote =
            record(&index, &walking, looked_at, &note, &content, Expect::NoRow).expect("record");

        assert!(!wrote);
        let query = writ_core::search::to_prefix_match("kestrel").expect("query");
        let terms = writ_core::search::search_terms("kestrel");
        assert_eq!(
            index.search_hits(&query, &terms, 10).expect("search").len(),
            1,
            "the saved text was replaced by what the walk read before the save"
        );
    }

    /// A walk that read a note before its folder moved writes nothing after
    /// the rows moved: the row would be keyed under the old folder, and
    /// search would answer with a note nothing can open.
    #[test]
    fn a_walk_writes_nothing_under_a_folder_that_moved_after_it_looked() {
        let (dir, index, notes) = store();
        let kept = notes.join("Kept.md");
        std::fs::write(&kept, "indexed before the move\n").expect("write");
        assert!(index.index_path(&kept).expect("index"));
        let fresh = notes.join("Fresh.md");
        std::fs::write(&fresh, "read by the walk\n").expect("write");

        let walking = Walking::start(index.recent_writes());
        let looked_at = walking.now();
        let (note, content) = read(&fresh);
        let moved = dir.path().join("moved");
        std::fs::rename(&notes, &moved).expect("move the folder");
        index.bump_generation();
        assert_eq!(index.rekey_root(&notes, &moved).expect("rekey"), 1);

        let wrote =
            record(&index, &walking, looked_at, &note, &content, Expect::NoRow).expect("record");

        assert!(!wrote);
        assert_eq!(
            indexed(&index),
            [crate::notes_index::index_key(&moved.join("Kept.md"))]
        );
    }

    /// Nothing is kept once the last walk ends, so a store that is not
    /// walking holds no record of its writes.
    #[test]
    fn the_record_of_writes_is_dropped_when_the_last_walk_ends() {
        let (_dir, index, notes) = store();
        let path = notes.join("Plan.md");
        std::fs::write(&path, "body\n").expect("write");

        let walking = Walking::start(index.recent_writes());
        let started = walking.started;
        assert!(index.index_path(&path).expect("index"));
        assert!(walking.overtaken(&crate::notes_index::index_key(&path), started));
        drop(walking);
        assert!(super::lock(index.recent_writes()).written.is_empty());

        assert!(index.index_path(&path).expect("index"));
        assert!(super::lock(index.recent_writes()).written.is_empty());
    }
}
