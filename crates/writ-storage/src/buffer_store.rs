use std::collections::HashMap;
use std::path::{Path, PathBuf};

use writ_core::file_ops::THRESHOLD_NORMAL_BYTES;

use rusqlite::Connection;
use tracing::warn;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::hash::Sha256Digest;
use writ_core::notes::guard::{decide_save, is_not_downloaded, DiskState, SaveDecision};
use writ_core::recovery::{
    fingerprint_buffers, should_snapshot, RecoveredBuffer, SnapshotFingerprint,
};

use crate::atomic::write_atomic;
use crate::database::queries;
use crate::errors::{StorageError, StorageResult};
use crate::maintenance::{self, DatabaseStats, MaintenanceOutcome};
use crate::notes_index;
use crate::recovery::dirty_shutdown::check_dirty_shutdown;
use crate::recovery::snapshot::SnapshotManager;

/// Called with a path and the bytes about to land on it, immediately before
/// every write this module performs.
///
/// The adapter passes one so a write of Writ's own is stamped in the watcher's
/// ignore set before it happens; without it, the folder's watcher reads the
/// write as somebody else's edit. `None` for a caller with no watcher, which
/// is every test and the CLI.
pub type BeforeWrite<'a> = Option<&'a dyn Fn(&Path, &[u8])>;

/// How a path's filesystem flags are read, for the check that keeps Writ from
/// pulling an evicted file down (`SF_DATALESS`, ADR-028 §5).
///
/// `None` asks the filesystem, which is what the app does. A test passes one
/// so it can exercise the flag on a machine where the flag cannot be set.
pub type DatalessProbe<'a> = Option<&'a dyn Fn(&Path) -> Option<u32>>;

/// What became of the text a relaunch recovered from the crash snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveredText {
    /// The file already held this text, or had never been written at all, and
    /// holds it now.
    Restored(DiskState),
    /// The file held something Writ never saw, or its bytes are not on this
    /// machine, so nothing was written where it was and the recovered text was
    /// written beside it.
    SetAside {
        /// What the note's file holds, which is not the recovered text.
        /// `None` for a file that was never read, which is what an evicted
        /// one is.
        on_disk: Option<DiskState>,
        /// Where the recovered text went instead.
        copy: PathBuf,
    },
}

impl RecoveredText {
    /// What the note's file holds, when that was established.
    ///
    /// The caller records this as the tab's disk state, so the first save
    /// after a relaunch is measured against the file. `None` records nothing,
    /// which leaves that first save to read the file itself — the honest
    /// outcome for a file Writ deliberately did not open.
    pub fn disk_state(&self) -> Option<DiskState> {
        match self {
            Self::Restored(state) => Some(*state),
            Self::SetAside { on_disk, .. } => *on_disk,
        }
    }
}

/// Persistence facade over note metadata and the files the notes live in.
///
/// The store owns a SQLite connection plus the retired mirror directory.
/// Metadata lives in the database; the text lives in the file named by
/// `source_path`, and that file is the only copy of it (ADR-028 §1). A row
/// with no `source_path` is a note that has not reached a file yet: it reads
/// as empty and cannot be written until one is attached.
///
/// `buffers_dir` no longer holds note text. It survives so the notes
/// migration can read what 0.3.5 left there and the consistency pass can
/// report anything it failed to clear.
pub struct BufferStore {
    conn: Connection,
    buffers_dir: PathBuf,
    /// The folder whose files the notes index holds (ADR-028 section 7).
    ///
    /// `None` until the adapter names it, which is the case in tests and
    /// one-shot tools that have no notes folder. A save then leaves the index
    /// alone and [`notes_index::reconcile`] owns it outright, so a store with
    /// no notes folder can never index a file that is not a note.
    notes_root: Option<PathBuf>,
    last_snapshot_fingerprint: Option<SnapshotFingerprint>,
}

impl BufferStore {
    /// Constructs a store over the given connection and mirror directory.
    pub fn new(conn: Connection, buffers_dir: PathBuf) -> Self {
        Self {
            conn,
            buffers_dir,
            notes_root: None,
            last_snapshot_fingerprint: None,
        }
    }

    /// Names the folder whose files this store keeps in the notes index.
    ///
    /// Writ's own writes are stamped into the watcher's ignore set before they
    /// land, so the notes watcher never sees them: without this the note a user
    /// just typed would stay unfindable until the next launch reconciled the
    /// folder. Set once, at startup, from the resolved notes folder.
    pub fn set_notes_root(&mut self, root: PathBuf) {
        self.notes_root = Some(root);
    }

    /// Returns the path to the retired mirror directory.
    pub fn buffers_dir(&self) -> &Path {
        &self.buffers_dir
    }

    /// Borrows the store's connection for the one-time passes that need to
    /// reach the database directly (the notes migration and the rollback
    /// bookkeeping beside it). Crate-internal: outside callers go through the
    /// facade.
    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Counts one launch against the pre-migration copy of the database and
    /// deletes it once it has survived `keep_launches` of them (ADR-028 §4).
    pub fn age_out_rollback_copy(&self, keep_launches: u32) -> StorageResult<bool> {
        crate::rollback::age_out_rollback_copy(&self.conn, keep_launches)
    }

    /// Inserts a new buffer row into the database.
    ///
    /// The search index is keyed by file path now, not by row (ADR-028
    /// section 7), so a row on its own indexes nothing: a new note enters the
    /// index when its file is written, through [`Self::save_to_source`] or the
    /// reconcile walk.
    pub fn insert(&self, doc: &BufferDocument) -> StorageResult<()> {
        queries::insert_buffer(&self.conn, doc)
    }

    /// Reads a buffer row by id.
    pub fn get(&self, id: &str) -> StorageResult<BufferDocument> {
        queries::get_buffer(&self.conn, id)
    }

    /// Marks the buffer as history and stamps its `closed_at`.
    pub fn close(&self, id: &str) -> StorageResult<()> {
        queries::close_buffer(&self.conn, id)
    }

    /// Closes every buffer listed in `ids` inside a single transaction.
    ///
    /// Missing or already-closed ids are silently no-ops at the SQL
    /// layer (the UPDATE matches zero rows). Atomicity guarantees that
    /// a mid-loop failure rolls back every prior close in the batch;
    /// the user never observes a partial close.
    pub fn close_many(&self, ids: &[String]) -> StorageResult<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        for id in ids {
            queries::close_buffer(&tx, id)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Restores a history buffer to active state.
    pub fn restore(&self, id: &str) -> StorageResult<()> {
        queries::restore_buffer(&self.conn, id)
    }

    /// Deletes the buffer row and its FTS row.
    ///
    /// **It never unlinks `source_path`.** Closing a tab, reclaiming an
    /// unwritten note and clearing history all reach this method, and none of
    /// them is a request to delete the note: the file on disk is the note
    /// (ADR-028 §1), and deleting a note is an explicit move to the Trash from
    /// the sidebar. The only file touched here is a mirror the notes migration
    /// left behind, cleared best-effort.
    ///
    /// The search index is untouched: closing a tab is not deleting a note, and
    /// the file the index holds is still on disk. A note leaves the index when
    /// its file does, through the notes watcher or the reconcile walk.
    pub fn delete(&self, id: &str) -> StorageResult<()> {
        let doc = queries::get_buffer(&self.conn, id)?;
        let mirror = self.buffers_dir.join(&doc.filename);
        if mirror.exists() {
            let _ = std::fs::remove_file(&mirror);
        }
        queries::delete_buffer(&self.conn, id)
    }

    /// Deletes every buffer listed in `ids` as a single all-or-nothing
    /// operation. Like [`Self::delete`], it leaves the search index alone
    /// and never
    /// unlinks a note's `source_path`; it only clears a mirror the notes
    /// migration left behind.
    ///
    /// Filenames are resolved before the transaction opens, so an unknown id
    /// aborts the whole batch before any row is touched. A mid-batch SQL
    /// failure rolls back every prior delete; the caller never observes a
    /// partially cleared set. Mirrors are removed only after the commit
    /// succeeds — filesystem deletes cannot enlist in the transaction.
    pub fn delete_many(&self, ids: &[String]) -> StorageResult<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut files = Vec::with_capacity(ids.len());
        for id in ids {
            let doc = queries::get_buffer(&self.conn, id)?;
            files.push(self.buffers_dir.join(&doc.filename));
        }
        let tx = self.conn.unchecked_transaction()?;
        for id in ids {
            queries::delete_buffer(&tx, id)?;
        }
        tx.commit()?;
        for file_path in &files {
            if file_path.exists() {
                let _ = std::fs::remove_file(file_path);
            }
        }
        Ok(())
    }

    /// Returns every buffer in the given status, ordered by tab position.
    pub fn list_by_status(&self, status: BufferStatus) -> StorageResult<Vec<BufferDocument>> {
        let status_str = match status {
            BufferStatus::Active => "active",
            BufferStatus::History => "history",
        };
        queries::list_buffers_by_status(&self.conn, status_str)
    }

    /// [`Self::save_to_source`] for a caller holding no record of the file.
    ///
    /// Passing no record is not a way around the guard: its question is
    /// whether the file changed since Writ last looked at it, and a caller
    /// with no record has not looked. The one caller in the app that is
    /// genuinely in that position is the relaunch after an unclean shutdown,
    /// which holds a snapshot of text and no record of the file it belongs to;
    /// it does not come through here but through
    /// [`Self::restore_recovered_content`], which compares against the file
    /// itself and sets the snapshot aside rather than writing over a version
    /// that arrived while Writ was down. What is left here is the plain write
    /// for callers with no watcher and nothing to lose: tests, benches and
    /// one-shot tools.
    pub fn save_content(&self, id: &str, content: &str) -> StorageResult<()> {
        self.save_to_source(id, content, None, None).map(|_| ())
    }

    /// [`Self::save_to_source_without_index`] for a caller holding no record
    /// of the file.
    pub fn save_content_without_index(&self, id: &str, content: &str) -> StorageResult<()> {
        self.save_to_source_without_index(id, content, None, None)
            .map(|_| ())
    }

    /// Refreshes the notes index for a single note from the file on disk (the
    /// reindex half of the deferred path, ADR-020).
    ///
    /// Reading the file rather than a captured string means a coalesced
    /// reindex always reflects the latest persisted bytes, so collapsing
    /// several edits into one reindex can never index a stale intermediate
    /// (ADR-028 section 12 keeps that argument and only changes which file is
    /// read). What changed with ADR-028 is the key and nothing else: the
    /// generation counter, the coalescing and the shutdown drain are the
    /// scheduler's, and they are untouched.
    ///
    /// Large-file and binary buffers (`size_bytes > THRESHOLD_NORMAL_BYTES`)
    /// are skipped, matching the write-time policy in [`Self::save_content`].
    /// A read-only row indexes nothing: search is over what the user wrote, and
    /// neither a hex view nor a document Writ generated is that.
    pub fn reindex_buffer(&self, id: &str) -> StorageResult<()> {
        let doc = queries::get_buffer(&self.conn, id)?;
        let content = Self::indexable_text(&doc);
        self.index_note(&doc, &content)
    }

    /// Records `doc`'s file in the notes index, when the row is one the index
    /// holds.
    ///
    /// A row is indexed when it has a file, that file is inside the notes
    /// folder, it is not read-only, and it opens with the full feature set. A
    /// read-only row is a hex view or a document Writ generated; a large or
    /// binary file is not text to search. Nothing here is fatal to a save: the
    /// bytes are already durable, and a file whose metadata cannot be read is
    /// picked up by the next reconcile.
    fn index_note(&self, doc: &BufferDocument, content: &str) -> StorageResult<()> {
        if doc.read_only || doc.size_bytes > THRESHOLD_NORMAL_BYTES {
            return Ok(());
        }
        let Some(source_path) = doc.source_path.as_deref() else {
            return Ok(());
        };
        let path = Path::new(source_path);
        if !self.is_within_notes(path) {
            return Ok(());
        }
        let Some(note) = notes_index::IndexedNote::from_file(path) else {
            return Ok(());
        };
        notes_index::NotesIndex::new(&self.conn).upsert(&note, content)
    }

    /// Whether `path` is inside the folder the notes index holds. Both sides
    /// go through [`notes_index::index_key`], so one file has one spelling on
    /// either side of the comparison.
    fn is_within_notes(&self, path: &Path) -> bool {
        let Some(root) = self.notes_root.as_deref() else {
            return false;
        };
        let root_key = notes_index::index_key(root);
        let path_key = notes_index::index_key(path);
        path_key
            .strip_prefix(&root_key)
            .is_some_and(|rest| rest.starts_with(['/', '\\']))
    }

    /// The text of `doc` as the index may hold it.
    ///
    /// Empty for a read-only row, whatever its file holds otherwise. A
    /// generated document must never enter the index no matter how often it
    /// is opened or rebuilt (ADR-028 §1), and a binary row's file is not text
    /// to search in the first place; both are read-only, and both keep the
    /// title-only entry [`Self::open_from_path_unindexed`] and [`Self::insert`]
    /// seed, so a rebuild leaves the index the same shape it found.
    fn indexable_text(doc: &BufferDocument) -> String {
        if doc.read_only {
            return String::new();
        }
        read_source_text(doc)
    }

    /// Reads a note's text from its file.
    ///
    /// A row with no `source_path` has not reached a file yet and reads as
    /// empty: there is nowhere else the text could be (ADR-028 §1). A
    /// read-only row is not always binary — a generated document (ADR-028
    /// §1's minted-nowhere case) is read-only text — so the hex view is only
    /// substituted when the bytes actually sniff as binary, regenerated from
    /// the file on every read rather than stored anywhere. Bytes that sniff
    /// as text but are not valid UTF-8 fall back to a lossy decode rather
    /// than failing the read.
    pub fn read_content(&self, id: &str) -> StorageResult<String> {
        let doc = queries::get_buffer(&self.conn, id)?;
        let Some(source_path) = doc.source_path.as_deref() else {
            return Ok(String::new());
        };
        if doc.read_only {
            let bytes = std::fs::read(source_path)?;
            if writ_core::file_ops::is_binary_bytes(&bytes) {
                return Ok(writ_core::file_ops::generate_hex_dump(
                    &bytes,
                    doc.size_bytes as usize,
                ));
            }
            return Ok(String::from_utf8_lossy(&bytes).into_owned());
        }
        Ok(std::fs::read_to_string(source_path)?)
    }

    /// Reads the bytes of a note's file.
    ///
    /// Reopening a file that is already open is how the user says "show me
    /// this file", so the caller reloads the editor from what is on disk
    /// rather than from what it read earlier.
    pub fn read_source(&self, id: &str) -> StorageResult<Vec<u8>> {
        let doc = queries::get_buffer(&self.conn, id)?;
        let source_path = doc
            .source_path
            .as_ref()
            .ok_or_else(|| StorageError::Consistency {
                message: format!("note {id} has no file"),
            })?;
        Ok(std::fs::read(source_path)?)
    }

    /// Renames a buffer's title and stamps `updated_at`.
    ///
    /// The search index is keyed by path and labelled by file name, so a title
    /// that changes without the file moving changes nothing it holds. The
    /// rename that does move the file is [`Self::rename_to_file`].
    pub fn rename(&self, id: &str, title: &str) -> StorageResult<()> {
        queries::rename_buffer(&self.conn, id, title)
    }

    /// Records a note's new file and the name that matches it, in one write.
    ///
    /// A rename moves the file and the name the tab shows together, so the two
    /// are written in one transaction alongside the index refresh: two writes
    /// leave a window where the row names one file and points at another, and
    /// a crash inside that window is a note nobody can find.
    ///
    /// `source_path` is where the file is now, so the index is re-keyed to it:
    /// the row under the old path goes, and the file is indexed under the new
    /// one. Writ's own rename is stamped into the watcher's ignore set, so the
    /// notes watcher will not do this for it.
    pub fn rename_to_file(&self, id: &str, source_path: &str, title: &str) -> StorageResult<()> {
        let doc = queries::get_buffer(&self.conn, id)?;
        let old_path = doc.source_path.clone();
        let renamed = BufferDocument {
            source_path: Some(source_path.to_string()),
            ..doc
        };
        let content = read_source_text(&renamed);

        let tx = self.conn.unchecked_transaction()?;
        queries::update_source_path(&tx, id, source_path)?;
        queries::rename_buffer(&tx, id, title)?;
        tx.commit()?;

        if let Some(old_path) = old_path {
            let old = Path::new(&old_path);
            if self.is_within_notes(old) {
                notes_index::NotesIndex::new(&self.conn).remove(&notes_index::index_key(old))?;
            }
        }
        self.index_note(&renamed, &content)
    }

    /// Updates the persistent tab order for a buffer.
    pub fn update_tab_order(&self, id: &str, order: u32) -> StorageResult<()> {
        queries::update_tab_order(&self.conn, id, order)
    }

    /// Finds the active buffer whose `source_path` matches, if any.
    pub fn find_active_by_source_path(
        &self,
        source_path: &str,
    ) -> StorageResult<Option<BufferDocument>> {
        queries::find_active_by_source_path(&self.conn, source_path)
    }

    /// Finds the most recently closed history buffer whose `source_path`
    /// matches, if any.
    pub fn find_history_by_source_path(
        &self,
        source_path: &str,
    ) -> StorageResult<Option<BufferDocument>> {
        queries::find_history_by_source_path(&self.conn, source_path)
    }

    /// Finds an active note that has not reached a file yet, suitable for
    /// reuse instead of minting a second one.
    ///
    /// A note acquires its file on the first keystroke (ADR-028 §2), so
    /// "has no file" is exactly "holds nothing the user typed" and needs no
    /// disk read. Callers must still flush any pending frontend autosave
    /// first: the file is attached by that save, so an unflushed keystroke
    /// leaves the row looking reusable.
    pub fn find_empty_scratch_active(&self) -> StorageResult<Option<BufferDocument>> {
        Ok(queries::list_unsaved_notes(&self.conn)?
            .into_iter()
            .find(|doc| doc.status == BufferStatus::Active))
    }

    /// Deletes every note that never reached a file, regardless of status,
    /// removing its row and its FTS entry. Returns the number reclaimed.
    ///
    /// Run once at startup to clear accumulated empty rows. Safe only when no
    /// note has unflushed content (true at launch), and only after the notes
    /// migration has run: a row it wrote into the archive keeps a `NULL`
    /// `source_path` and is excluded by its `migrated_path`.
    pub fn reclaim_empty_scratch(&self) -> StorageResult<usize> {
        let candidates = queries::list_unsaved_notes(&self.conn)?;
        let mut reclaimed = 0;
        for doc in candidates {
            self.delete(&doc.id)?;
            reclaimed += 1;
        }
        Ok(reclaimed)
    }

    /// Attaches a file to a note that had none, and stamps `updated_at`.
    ///
    /// This is how a new note acquires the file the invariant requires
    /// (ADR-028 §2). Fails with `Consistency` when the row already has one:
    /// moving a note to another path is [`Self::update_source_path`], and
    /// silently repointing a note at a second file is how a note gets lost.
    pub fn attach_source_path(&self, id: &str, source_path: &str) -> StorageResult<()> {
        let doc = queries::get_buffer(&self.conn, id)?;
        if let Some(existing) = doc.source_path {
            return Err(StorageError::Consistency {
                message: format!("note {id} already has a file at {existing}"),
            });
        }
        queries::update_source_path(&self.conn, id, source_path)?;
        queries::update_timestamp(&self.conn, id)
    }

    /// Rewrites a note's file path after the file moved or was renamed.
    pub fn update_source_path(&self, id: &str, source_path: &str) -> StorageResult<()> {
        queries::update_source_path(&self.conn, id, source_path)
    }

    /// Opens a note that originated from an external file, inserting its row
    /// and recording its file in the notes index.
    ///
    /// Nothing is written to disk: the file the row points at already holds
    /// `content`, and it is the only copy of it (ADR-028 section 1). A file
    /// outside the notes folder, over [`THRESHOLD_NORMAL_BYTES`], or read-only
    /// is not indexed.
    pub fn open_from_path(&self, doc: &BufferDocument, content: &str) -> StorageResult<()> {
        queries::insert_buffer(&self.conn, doc)?;
        self.index_note(doc, content)
    }

    /// [`Self::open_from_path`] without ever indexing the file's text.
    ///
    /// For a generated document (ADR-028 section 1): its text is not the user's
    /// writing, so it must never enter the search index no matter how small the
    /// file is, or where it sits.
    pub fn open_from_path_unindexed(&self, doc: &BufferDocument) -> StorageResult<()> {
        queries::insert_buffer(&self.conn, doc)
    }

    /// Persists content back to the note's file, refreshing the search index.
    ///
    /// `last_known` is what the caller recorded the last time it read or wrote
    /// this file; the write is refused when the file has changed since
    /// ([`Self::write_source_guarded`]). Returns what the file holds
    /// afterwards, which the caller records for the next save.
    ///
    /// The file is written first, then the `updated_at` stamp, then the index.
    /// Index errors propagate rather than being swallowed; a crash that still
    /// slips the index out of sync is healed at the next launch by
    /// [`notes_index::reconcile`], which compares every row against the file
    /// behind it.
    ///
    /// Buffers with `size_bytes > THRESHOLD_NORMAL_BYTES` (large-file and
    /// binary tiers) are excluded from indexing: the cost of indexing a
    /// 50 MB log degrades search for all of them with no practical benefit.
    /// A read-only row is refused (a binary hex view must never write back
    /// over its file).
    pub fn save_to_source(
        &self,
        id: &str,
        content: &str,
        last_known: Option<DiskState>,
        before_write: BeforeWrite<'_>,
    ) -> StorageResult<DiskState> {
        let (doc, state) = self.write_source_guarded(id, content, last_known, before_write)?;

        queries::update_timestamp(&self.conn, id)?;
        self.index_note(&doc, content)?;
        Ok(state)
    }

    /// [`Self::save_to_source`] **without** touching the search index.
    ///
    /// The write half of the deferred-reindex path (ADR-020): autosave writes
    /// through here on every idle window, then schedules one coalesced
    /// [`Self::reindex_buffer`] so the FTS cost leaves the keystroke loop. The
    /// bytes are durable on return; only search freshness lags.
    pub fn save_to_source_without_index(
        &self,
        id: &str,
        content: &str,
        last_known: Option<DiskState>,
        before_write: BeforeWrite<'_>,
    ) -> StorageResult<DiskState> {
        let (_doc, state) = self.write_source_guarded(id, content, last_known, before_write)?;
        queries::update_timestamp(&self.conn, id)?;
        Ok(state)
    }

    /// Writes `content` to the note's file unless doing so would lose a change
    /// Writ never read, returning the row it resolved from and what the file
    /// holds afterwards.
    ///
    /// Every write path in the store funnels through here, so there is one
    /// answer to where a note's text goes, one place the invariant can be
    /// broken, and one guard in front of it. A row with no `source_path` is
    /// rejected rather than written anywhere else: the caller has to attach a
    /// file first ([`Self::attach_source_path`]).
    ///
    /// The decision is [`decide_save`]'s. When it stops the write, the
    /// incoming text is written beside the note as a dated copy first, so a
    /// refusal never ends with the user's text nowhere (ADR-028 §5), and the
    /// error names where it went.
    ///
    /// A file whose bytes are not on this machine is stopped before the
    /// compare read, because that read is what would pull it down.
    ///
    /// # Errors
    ///
    /// [`StorageError::SourceChangedOnDisk`] when the file changed under Writ
    /// and holds something other than what is being written;
    /// [`StorageError::SourceNotDownloaded`] when the file has not finished
    /// downloading;
    /// [`StorageError::Consistency`] for a read-only row or one with no file;
    /// [`StorageError::Io`] when the file cannot be read or written.
    pub fn write_source_guarded(
        &self,
        id: &str,
        content: &str,
        last_known: Option<DiskState>,
        before_write: BeforeWrite<'_>,
    ) -> StorageResult<(BufferDocument, DiskState)> {
        let doc = queries::get_buffer(&self.conn, id)?;
        if doc.read_only {
            return Err(StorageError::Consistency {
                message: format!("note {id} is read-only and cannot be saved"),
            });
        }
        let source_path = doc
            .source_path
            .as_ref()
            .ok_or_else(|| StorageError::Consistency {
                message: format!("note {id} has no file to save into"),
            })?
            .clone();
        let path = Path::new(&source_path);

        if is_not_downloaded(dataless_flags(path)) {
            return Err(StorageError::SourceNotDownloaded { path: source_path });
        }

        let incoming = writ_core::hash::sha256_bytes(content.as_bytes());
        let on_disk = read_disk_state(path)?;
        let decision = decide_save(last_known.as_ref(), on_disk.as_ref(), incoming);

        // The two decisions that stop a write can only come from a file that
        // is there, so the arms that read one are the only ones that need it.
        // A `None` alongside either could only mean the file went missing
        // between the two reads, which proceeds.
        match (decision, on_disk) {
            (SaveDecision::AlreadyIdentical, Some(state)) => Ok((doc, state)),
            (SaveDecision::Refuse, Some(state)) => {
                let conflict_copy = match write_conflict_copy(
                    path,
                    content,
                    chrono::Utc::now(),
                    before_write,
                ) {
                    Ok(written) => Some(written.to_string_lossy().into_owned()),
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "the copy beside the note could not be written");
                        None
                    }
                };
                Err(StorageError::SourceChangedOnDisk {
                    path: source_path,
                    disk_hash: writ_core::hash::digest_hex(state.hash),
                    conflict_copy,
                })
            }
            _ => {
                write_guarded_by_stamp(path, content.as_bytes(), before_write)?;
                Ok((doc, written_state(path, incoming, content.len() as u64)))
            }
        }
    }

    /// Writes text recovered from the crash snapshot into the note's file,
    /// unless the file moved on while Writ was down.
    ///
    /// The relaunch is the one caller that holds text and no record of the
    /// file it belongs to, so [`decide_save`] has nothing to compare and would
    /// proceed. It must not: a sync client can deliver a newer version of a
    /// note between the crash and the relaunch, and writing a pre-crash
    /// snapshot over it destroys work with nothing left to recover it from.
    ///
    /// A file that already holds the recovered text is left alone and reported
    /// as restored, because it is: rewriting identical bytes only moves the
    /// modification time. A file that holds anything else is left untouched
    /// too, and the recovered text is written beside it as
    /// `<stem> (recovered YYYY-MM-DD HH.MM.SS)`, so both are on disk and the
    /// user chooses.
    ///
    /// A path with no file at it is written. The only caller that reaches
    /// here with one is a note whose path was minted a moment ago, because a
    /// note that had a file and lost it never gets this far
    /// ([`writ_core::recovery::plan_recovery`]): a relaunch does not undo a
    /// deletion, and it leaves nothing beside the path either.
    ///
    /// A file whose bytes are not on this machine is never opened. Reading one
    /// makes the provider daemon fetch it (ADR-028 §5), and a relaunch that
    /// pulled down every evicted note to compare it against a snapshot would
    /// be the worst moment to do that. It takes the same route as a file that
    /// moved on: left alone, snapshot beside it, and nothing recorded about
    /// what it holds, so the first save reads it then.
    ///
    /// The index and the `updated_at` stamp are left alone: the relaunch
    /// reindexes what it restored through the ordinary path, and a note whose
    /// text was set aside has not changed.
    ///
    /// # Errors
    ///
    /// [`StorageError::Consistency`] for a read-only row or one with no file,
    /// and [`StorageError::Io`] when the file cannot be read or written.
    pub fn restore_recovered_content(
        &self,
        id: &str,
        content: &str,
        before_write: BeforeWrite<'_>,
        dataless: DatalessProbe<'_>,
    ) -> StorageResult<RecoveredText> {
        let doc = queries::get_buffer(&self.conn, id)?;
        if doc.read_only {
            return Err(StorageError::Consistency {
                message: format!("note {id} is read-only and cannot be saved"),
            });
        }
        let source_path = doc
            .source_path
            .as_ref()
            .ok_or_else(|| StorageError::Consistency {
                message: format!("note {id} has no file to save into"),
            })?
            .clone();
        let path = Path::new(&source_path);

        let flags = match dataless {
            Some(probe) => probe(path),
            None => dataless_flags(path),
        };
        if is_not_downloaded(flags) {
            let copy = write_recovered_copy(path, content, chrono::Utc::now(), before_write)?;
            warn!(
                note = %path.display(),
                recovered = %copy.display(),
                "the file has not finished downloading; the recovered text was written beside it"
            );
            return Ok(RecoveredText::SetAside {
                on_disk: None,
                copy,
            });
        }

        let incoming = writ_core::hash::sha256_bytes(content.as_bytes());
        let on_disk = read_disk_state(path)?;

        if let Some(state) = on_disk {
            if state.hash != incoming {
                let copy = write_recovered_copy(path, content, chrono::Utc::now(), before_write)?;
                warn!(
                    note = %path.display(),
                    recovered = %copy.display(),
                    "the file moved on while Writ was down; the recovered text was written beside it"
                );
                return Ok(RecoveredText::SetAside {
                    on_disk: Some(state),
                    copy,
                });
            }
            // The file already holds the recovered text, so there is nothing
            // to restore. Writing it anyway would move the modification time
            // and swap the inode for bytes that did not change, which a sync
            // client reads as an edit and uploads. Same answer the save path
            // gives ([`SaveDecision::AlreadyIdentical`]).
            return Ok(RecoveredText::Restored(state));
        }

        write_guarded_by_stamp(path, content.as_bytes(), before_write)?;
        Ok(RecoveredText::Restored(written_state(
            path,
            incoming,
            content.len() as u64,
        )))
    }

    /// Updates the detected or user-assigned language for a buffer.
    pub fn update_language(&self, id: &str, language: Option<&str>) -> StorageResult<()> {
        queries::update_language(&self.conn, id, language)
    }

    /// Normalizes every buffer's mirror `filename` to `{id}.txt` and then
    /// installs a `UNIQUE` index on `buffers(filename)`.
    ///
    /// Legacy rows minted before audit blocker #53.7 derived their mirror
    /// filename from the file's basename, so two files sharing a basename
    /// could overwrite each other's backing content. This one-time, idempotent
    /// reconciliation renames each legacy backing file to its UUID-derived
    /// name, rewrites the row, and only then creates the uniqueness index —
    /// uniqueness cannot be a SQL migration because the index must be built
    /// *after* the on-disk files are moved, which SQL cannot do.
    ///
    /// A missing backing file is tolerated (the original collision may have
    /// already consumed it); the row is still normalized so no future write
    /// targets the colliding name. Returns the number of rows reconciled.
    pub fn reconcile_buffer_filenames(&self) -> StorageResult<usize> {
        let mut docs = self.list_by_status(BufferStatus::Active)?;
        docs.extend(self.list_by_status(BufferStatus::History)?);

        let mut reconciled = 0;
        for doc in &docs {
            let target = format!("{}.txt", doc.id);
            if doc.filename == target {
                continue;
            }
            let old_path = self.buffers_dir.join(&doc.filename);
            let new_path = self.buffers_dir.join(&target);
            if old_path.exists() && !new_path.exists() {
                std::fs::rename(&old_path, &new_path)?;
            } else if !new_path.exists() {
                // No backing file to move: a prior collision (two rows sharing
                // one mirror filename) already consumed it. Normalize the row
                // anyway so no future write targets the colliding name, but
                // surface the silent content loss.
                warn!(
                    buffer_id = %doc.id,
                    filename = %doc.filename,
                    "reconcile: legacy buffer has no backing file (lost to a prior filename collision)"
                );
            }
            queries::update_filename(&self.conn, &doc.id, &target)?;
            reconciled += 1;
        }

        self.conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_buffers_filename ON buffers(filename)",
            [],
        )?;

        Ok(reconciled)
    }

    /// Returns `true` when the most recent session snapshot was not written
    /// with a clean flag, indicating the previous run crashed or was
    /// force-quit.
    pub fn is_dirty_shutdown(&self) -> StorageResult<bool> {
        check_dirty_shutdown(&self.conn)
    }

    /// Writes a session snapshot containing the given buffer contents.
    ///
    /// Snapshots are pruned to the retention limit after each write. Pass
    /// `is_clean = true` on a graceful shutdown; pass `false` for periodic
    /// heartbeat snapshots written while the app is running.
    pub fn write_session_snapshot(
        &self,
        buffer_contents: &HashMap<String, String>,
        is_clean: bool,
    ) -> StorageResult<()> {
        let extra = serde_json::Value::Object(serde_json::Map::new());
        let mgr = SnapshotManager::new(&self.conn);
        mgr.write_session_snapshot(buffer_contents, &extra, is_clean)
    }

    /// Writes a heartbeat snapshot only when the buffer contents differ from
    /// the last snapshot this store wrote.
    ///
    /// Returns `true` when a snapshot was written. The unconditional variant
    /// stays available for shutdown, where the `is_clean` marker itself is the
    /// payload and must be recorded whatever the content is.
    pub fn write_session_snapshot_if_changed(
        &mut self,
        buffer_contents: &HashMap<String, String>,
    ) -> StorageResult<bool> {
        let fingerprint = fingerprint_buffers(buffer_contents);
        if !should_snapshot(self.last_snapshot_fingerprint, fingerprint) {
            return Ok(false);
        }
        self.write_session_snapshot(buffer_contents, false)?;
        self.last_snapshot_fingerprint = Some(fingerprint);
        Ok(true)
    }

    /// Returns page accounting for the underlying database file.
    pub fn database_stats(&self) -> StorageResult<DatabaseStats> {
        maintenance::read_stats(&self.conn)
    }

    /// Checkpoints the write-ahead log and reclaims free pages when they
    /// dominate the database file.
    pub fn run_maintenance(&self) -> StorageResult<MaintenanceOutcome> {
        maintenance::run_maintenance(&self.conn)
    }

    /// Resolves which buffers should be restored from the latest dirty
    /// snapshot.
    ///
    /// Reads current `updated_at` timestamps from the database, then
    /// delegates to [`SnapshotManager::recover_buffers`].
    ///
    /// Closed notes are considered as well as open ones. A note whose save was
    /// refused and whose tab was then closed hands its text to the shutdown
    /// snapshot on the way out; the row has moved to `history` by then, and
    /// looking at open notes alone would drop the one entry the snapshot was
    /// written for. A note whose row is gone entirely is still dropped: there
    /// is no file left to write it to.
    pub fn resolve_recovery(&self) -> StorageResult<Vec<RecoveredBuffer>> {
        let mut updated_at_map: HashMap<String, String> = HashMap::new();
        for status in [BufferStatus::Active, BufferStatus::History] {
            for buf in self.list_by_status(status)? {
                updated_at_map.insert(
                    buf.id.clone(),
                    buf.updated_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                );
            }
        }
        let mgr = SnapshotManager::new(&self.conn);
        mgr.recover_buffers(&updated_at_map)
    }

    /// Collects the current on-disk content of every active note.
    ///
    /// Notes that have not reached a file, and notes whose file cannot be
    /// read, are silently skipped; the snapshot simply contains fewer
    /// entries.
    ///
    /// Buffers in the large-file or binary tiers (`size_bytes >
    /// THRESHOLD_NORMAL_BYTES`) are excluded. Reading hundreds of megabytes in
    /// the periodic heartbeat would spike RAM and provide little recovery
    /// value (the file still exists on disk).
    pub fn collect_buffer_contents(&self) -> StorageResult<HashMap<String, String>> {
        let active = self.list_by_status(BufferStatus::Active)?;
        let mut map = HashMap::new();
        for buf in active {
            if buf.size_bytes > THRESHOLD_NORMAL_BYTES {
                continue;
            }
            let Some(source_path) = buf.source_path.as_deref() else {
                continue;
            };
            if let Ok(content) = std::fs::read_to_string(source_path) {
                map.insert(buf.id, content);
            }
        }
        Ok(map)
    }
}

/// A note's text as the index wants it: whatever the file holds, or nothing
/// when the note has no file yet or its file has gone missing.
///
/// The index tolerates both. A note with no file holds no text to index, and
/// a file that vanished under an open tab is recreated by the next save, so
/// neither is a reason to fail a reindex.
/// The filesystem flags on `path`, or `None` when the platform has none to
/// report or the file is gone.
///
/// macOS is the only platform with `SF_DATALESS`; everywhere else this is a
/// constant `None` and the check that reads it folds away.
#[cfg(target_os = "macos")]
pub fn dataless_flags(path: &Path) -> Option<u32> {
    use std::os::macos::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.st_flags())
}

/// [`dataless_flags`] on a platform with no such flag.
#[cfg(not(target_os = "macos"))]
pub fn dataless_flags(_path: &Path) -> Option<u32> {
    None
}

/// A file read once, answering both questions asked of it.
pub struct NoteFileState {
    /// What the write guard compares: the digest of the bytes as they are,
    /// line endings and all, plus the size and modification time.
    pub disk: DiskState,
    /// What the editor compares its document against
    /// ([`writ_core::hash::comparison_digest_hex`]).
    pub comparison_hash: String,
}

/// What `path` holds right now, or `None` when there is no file there.
///
/// One read answers both: a second read to compute the other digest could see
/// a different version of a file being written while this runs, and the two
/// answers would then describe two different files.
pub fn read_note_file_state(path: &Path) -> StorageResult<Option<NoteFileState>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let metadata = std::fs::metadata(path).ok();
    Ok(Some(NoteFileState {
        comparison_hash: writ_core::hash::comparison_digest_hex(&bytes),
        disk: DiskState {
            hash: writ_core::hash::sha256_bytes(&bytes),
            size: metadata
                .as_ref()
                .map(|m| m.len())
                .unwrap_or(bytes.len() as u64),
            mtime: metadata.as_ref().and_then(|m| m.modified().ok()),
        },
    }))
}

/// What `path` holds right now, for the write guard alone.
pub fn read_disk_state(path: &Path) -> StorageResult<Option<DiskState>> {
    Ok(read_note_file_state(path)?.map(|state| state.disk))
}

/// The state of a file just written, without reading it back.
///
/// The digest and the length are what was written; only the modification time
/// has to come from the filesystem, and a file whose metadata cannot be read
/// records none rather than failing a save that already landed.
fn written_state(path: &Path, hash: Sha256Digest, size: u64) -> DiskState {
    DiskState {
        hash,
        size,
        mtime: std::fs::metadata(path).ok().and_then(|m| m.modified().ok()),
    }
}

/// The names `dir` already holds, lowercased the way the dedupe compares them.
pub(crate) fn taken_names(dir: &Path) -> std::collections::HashSet<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return std::collections::HashSet::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().to_lowercase())
        .collect()
}

/// Writes `content` beside `note_path` as a dated copy and returns the path
/// written.
///
/// This is what keeps a refused save from ending with the user's text nowhere
/// (ADR-028 §5). The name comes from [`writ_core::notes::conflict_file_name`]
/// and dedupes Finder-style, so two refusals inside the same second produce
/// two files rather than one overwriting the other.
///
/// # Errors
///
/// [`StorageError::Consistency`] when the note has no folder to be written
/// beside, and [`StorageError::Io`] when the copy cannot be written.
pub fn write_conflict_copy(
    note_path: &Path,
    content: &str,
    now: chrono::DateTime<chrono::Utc>,
    before_write: BeforeWrite<'_>,
) -> StorageResult<PathBuf> {
    write_beside(
        note_path,
        content,
        before_write,
        |stem, now| writ_core::notes::conflict_file_name(stem, "", now),
        now,
    )
}

/// Writes `content` beside `note_path` as a dated copy the crash snapshot was
/// holding, and returns the path written.
///
/// The relaunch counterpart of [`write_conflict_copy`]: same folder, same
/// dedupe, a name that says where the text came from
/// ([`writ_core::notes::recovered_file_name`]).
///
/// # Errors
///
/// [`StorageError::Consistency`] when the note has no folder to be written
/// beside, and [`StorageError::Io`] when the copy cannot be written.
pub fn write_recovered_copy(
    note_path: &Path,
    content: &str,
    now: chrono::DateTime<chrono::Utc>,
    before_write: BeforeWrite<'_>,
) -> StorageResult<PathBuf> {
    write_beside(
        note_path,
        content,
        before_write,
        |stem, now| writ_core::notes::recovered_file_name(stem, "", now),
        now,
    )
}

/// The shared half of both dated copies: name from `name_stem`, dedupe against
/// the folder, stamp, write.
fn write_beside(
    note_path: &Path,
    content: &str,
    before_write: BeforeWrite<'_>,
    name_stem: impl Fn(&str, chrono::DateTime<chrono::Utc>) -> String,
    now: chrono::DateTime<chrono::Utc>,
) -> StorageResult<PathBuf> {
    let dir = note_path
        .parent()
        .ok_or_else(|| StorageError::Consistency {
            message: format!("{} has no folder to be written beside", note_path.display()),
        })?;
    let stem = note_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = note_path
        .extension()
        .map(|ext| ext.to_string_lossy().into_owned())
        .unwrap_or_default();

    let name =
        writ_core::notes::dedupe_file_name(&name_stem(&stem, now), &extension, &taken_names(dir));
    let target = dir.join(name);
    write_guarded_by_stamp(&target, content.as_bytes(), before_write)?;
    Ok(target)
}

/// Stamps then writes, in that order.
///
/// Every write this crate performs goes through here, because a write the
/// caller has not been told about first is a write its watcher reads as
/// somebody else's edit. [`write_atomic`] has this one call site so that no
/// future write can skip the stamp by reaching past it.
pub(crate) fn write_guarded_by_stamp(
    target: &Path,
    bytes: &[u8],
    before_write: BeforeWrite<'_>,
) -> std::io::Result<()> {
    if let Some(stamp) = before_write {
        stamp(target, bytes);
    }
    write_atomic(target, bytes)
}

fn read_source_text(doc: &BufferDocument) -> String {
    doc.source_path
        .as_deref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default()
}
