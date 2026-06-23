use std::collections::HashMap;
use std::path::{Path, PathBuf};

use writ_core::file_ops::THRESHOLD_NORMAL_BYTES;

use rusqlite::Connection;
use tracing::warn;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::recovery::RecoveredBuffer;
use writ_core::search::SearchHit;

use crate::atomic::write_atomic;
use crate::database::queries;
use crate::errors::{StorageError, StorageResult};
use crate::recovery::dirty_shutdown::check_dirty_shutdown;
use crate::recovery::snapshot::SnapshotManager;

/// Persistence facade over buffer metadata and on-disk content.
///
/// The store owns a SQLite connection plus a per-Writ buffers directory.
/// Buffer metadata lives in the database; buffer content lives as a
/// plain file named after the buffer's `filename`. Both are kept in
/// sync through the FTS index on write and delete.
pub struct BufferStore {
    conn: Connection,
    buffers_dir: PathBuf,
}

impl BufferStore {
    /// Constructs a store over the given connection and buffers directory.
    pub fn new(conn: Connection, buffers_dir: PathBuf) -> Self {
        Self { conn, buffers_dir }
    }

    /// Returns the path to the directory holding buffer content files.
    pub fn buffers_dir(&self) -> &Path {
        &self.buffers_dir
    }

    /// Inserts a new buffer row into the database and seeds its FTS entry.
    ///
    /// Seeding an (empty-content) FTS row at insert time makes the
    /// FTS-vs-buffers parity invariant structural rather than dependent on
    /// every caller remembering to follow up with a content write: an
    /// indexed-eligible buffer is in the index from the moment it exists,
    /// so [`Self::verify_and_repair_fts`] never sees a freshly inserted row
    /// as drift. Large-file and binary buffers (`size_bytes >
    /// THRESHOLD_NORMAL_BYTES`) are excluded, matching every other write
    /// site.
    pub fn insert(&self, doc: &BufferDocument) -> StorageResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        queries::insert_buffer(&tx, doc)?;
        if doc.size_bytes <= THRESHOLD_NORMAL_BYTES {
            crate::fts::FtsIndex::new(&tx).insert(&doc.id, &doc.title, "")?;
        }
        tx.commit()?;
        Ok(())
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

    /// Deletes the buffer row, its backing content file, and its FTS
    /// row.
    ///
    /// The FTS row is removed first while the buffer row still exists
    /// (the FTS lookup is keyed off `buffers.rowid`), then the content
    /// file, then the buffer row itself. Failures to remove the FTS row
    /// or content file are not allowed to block the database deletion:
    /// the on-disk artifact is best-effort, but losing the buffer row
    /// without losing the FTS row is what produces orphan hits, so the
    /// FTS step propagates errors.
    pub fn delete(&self, id: &str) -> StorageResult<()> {
        let doc = queries::get_buffer(&self.conn, id)?;
        let fts = crate::fts::FtsIndex::new(&self.conn);
        fts.delete(id)?;
        let file_path = self.buffers_dir.join(&doc.filename);
        if file_path.exists() {
            std::fs::remove_file(&file_path)?;
        }
        queries::delete_buffer(&self.conn, id)
    }

    /// Deletes every buffer listed in `ids` — rows, FTS entries, and backing
    /// content files — as a single all-or-nothing operation.
    ///
    /// Filenames are resolved before the transaction opens, so an unknown id
    /// aborts the whole batch before any row is touched. A mid-batch SQL
    /// failure rolls back every prior delete; the caller never observes a
    /// partially cleared set. Content files are removed only after the commit
    /// succeeds — filesystem deletes cannot enlist in the transaction — so a
    /// file-removal failure can orphan a file but never leaves a half-deleted
    /// database.
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
        {
            let fts = crate::fts::FtsIndex::new(&tx);
            for id in ids {
                fts.delete(id)?;
                queries::delete_buffer(&tx, id)?;
            }
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

    /// Writes `content` to the buffer's backing file and refreshes the
    /// FTS index.
    ///
    /// The content file is written atomically first. The `updated_at`
    /// stamp and the FTS row are then updated inside a single transaction
    /// (audit blocker #53.5): either both land or neither does, so the
    /// timestamp can never advance past a stale index. FTS errors
    /// propagate rather than being swallowed; a crash that still slips the
    /// index out of sync is healed at the next boot by
    /// [`Self::verify_and_repair_fts`].
    ///
    /// Buffers with `size_bytes > THRESHOLD_NORMAL_BYTES` (large-file
    /// and binary tiers) are excluded from FTS indexing: the cost of
    /// indexing a 50 MiB log degrades search for all buffers with no
    /// practical benefit.
    pub fn save_content(&self, id: &str, content: &str) -> StorageResult<()> {
        let doc = queries::get_buffer(&self.conn, id)?;
        if doc.read_only {
            return Err(crate::errors::StorageError::Consistency {
                message: format!("buffer {} is read-only and cannot be saved", id),
            });
        }
        let file_path = self.buffers_dir.join(&doc.filename);
        write_atomic(&file_path, content.as_bytes())?;

        let tx = self.conn.unchecked_transaction()?;
        queries::update_timestamp(&tx, id)?;
        if doc.size_bytes <= THRESHOLD_NORMAL_BYTES {
            crate::fts::FtsIndex::new(&tx).update(id, &doc.title, content)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Writes `content` to the buffer's backing file and stamps `updated_at`,
    /// **without** touching the FTS index.
    ///
    /// This is the write half of the deferred-reindex path (ADR-020): the IPC
    /// autosave command writes immediately through this method, then schedules
    /// a coalesced [`Self::reindex_buffer`] a short time later so the FTS cost
    /// leaves the keystroke loop. Durability of the bytes on disk is identical
    /// to [`Self::save_content`]; only the index is deferred. Callers that need
    /// search to reflect the write immediately must use [`Self::save_content`].
    pub fn save_content_without_index(&self, id: &str, content: &str) -> StorageResult<()> {
        let doc = queries::get_buffer(&self.conn, id)?;
        if doc.read_only {
            return Err(crate::errors::StorageError::Consistency {
                message: format!("buffer {} is read-only and cannot be saved", id),
            });
        }
        let file_path = self.buffers_dir.join(&doc.filename);
        write_atomic(&file_path, content.as_bytes())?;
        queries::update_timestamp(&self.conn, id)?;
        Ok(())
    }

    /// Rebuilds the FTS row for a single buffer from its current title and
    /// on-disk content (the reindex half of the deferred path, ADR-020).
    ///
    /// Reading content from disk rather than from a captured string means a
    /// coalesced reindex always reflects the latest persisted bytes, so
    /// collapsing several edits into one reindex can never index a stale
    /// intermediate. Large-file and binary buffers
    /// (`size_bytes > THRESHOLD_NORMAL_BYTES`) are skipped, matching the
    /// write-time policy in [`Self::save_content`].
    pub fn reindex_buffer(&self, id: &str) -> StorageResult<()> {
        let doc = queries::get_buffer(&self.conn, id)?;
        if doc.size_bytes > THRESHOLD_NORMAL_BYTES {
            return Ok(());
        }
        let file_path = self.buffers_dir.join(&doc.filename);
        let content = if file_path.exists() {
            std::fs::read_to_string(&file_path).unwrap_or_default()
        } else {
            String::new()
        };
        let fts = crate::fts::FtsIndex::new(&self.conn);
        fts.update(id, &doc.title, &content)
    }

    /// Reads the textual content of a buffer from its backing file.
    pub fn read_content(&self, id: &str) -> StorageResult<String> {
        let doc = queries::get_buffer(&self.conn, id)?;
        let file_path = self.buffers_dir.join(&doc.filename);
        if !file_path.exists() {
            return Err(StorageError::Consistency {
                message: format!("content file not found for buffer: {}", id),
            });
        }
        let content = std::fs::read_to_string(&file_path)?;
        Ok(content)
    }

    /// Renames a buffer's title, stamps `updated_at`, and refreshes the
    /// FTS index so searches against the new title hit immediately.
    ///
    /// The rename and the FTS refresh run in a single transaction and FTS
    /// errors propagate, matching [`Self::save_content`]: the title and
    /// the index never diverge.
    pub fn rename(&self, id: &str, title: &str) -> StorageResult<()> {
        let doc = queries::get_buffer(&self.conn, id)?;
        let file_path = self.buffers_dir.join(&doc.filename);
        let content = if file_path.exists() {
            std::fs::read_to_string(&file_path).unwrap_or_default()
        } else {
            String::new()
        };

        let tx = self.conn.unchecked_transaction()?;
        queries::rename_buffer(&tx, id, title)?;
        if doc.size_bytes <= THRESHOLD_NORMAL_BYTES {
            crate::fts::FtsIndex::new(&tx).update(id, title, &content)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Updates the persistent tab order for a buffer.
    pub fn update_tab_order(&self, id: &str, order: u32) -> StorageResult<()> {
        queries::update_tab_order(&self.conn, id, order)
    }

    /// Runs a full-text search and returns matching buffer ids in
    /// relevance order.
    pub fn search(&self, query: &str) -> StorageResult<Vec<String>> {
        let fts = crate::fts::FtsIndex::new(&self.conn);
        fts.search(query)
    }

    /// Runs a full-text search and returns up to `limit` display hits (title,
    /// matching line, highlighted snippet) in relevance order.
    pub fn search_hits(
        &self,
        query: &str,
        terms: &[String],
        limit: usize,
    ) -> StorageResult<Vec<SearchHit>> {
        let fts = crate::fts::FtsIndex::new(&self.conn);
        fts.search_hits(query, terms, limit)
    }

    /// Returns the total number of buffers matching `query`, ignoring any limit.
    pub fn search_count(&self, query: &str) -> StorageResult<usize> {
        let fts = crate::fts::FtsIndex::new(&self.conn);
        fts.count(query)
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

    /// Reports whether a buffer's backing content file is empty. A
    /// missing file counts as empty (a freshly inserted buffer before
    /// its first content write).
    fn is_empty_on_disk(&self, doc: &BufferDocument) -> bool {
        let file_path = self.buffers_dir.join(&doc.filename);
        match std::fs::metadata(&file_path) {
            Ok(meta) => meta.len() == 0,
            Err(_) => true,
        }
    }

    /// Finds an active, never-renamed scratch buffer with no content,
    /// suitable for reuse instead of minting a new empty buffer.
    ///
    /// "Empty" is read from disk; callers must flush any pending
    /// frontend autosave before relying on this, since content lives on
    /// disk and trails the live editor by the autosave debounce window.
    pub fn find_empty_scratch_active(&self) -> StorageResult<Option<BufferDocument>> {
        let candidates = queries::list_scratch_candidates(&self.conn)?;
        Ok(candidates
            .into_iter()
            .find(|doc| doc.status == BufferStatus::Active && self.is_empty_on_disk(doc)))
    }

    /// Deletes every empty, never-renamed scratch buffer regardless of
    /// status, removing its row, backing file, and FTS entry. Returns
    /// the number reclaimed.
    ///
    /// Run once at startup to clear accumulated empty scratch rows.
    /// Safe only when no buffer has unflushed content (true at launch).
    pub fn reclaim_empty_scratch(&self) -> StorageResult<usize> {
        let candidates = queries::list_scratch_candidates(&self.conn)?;
        let mut reclaimed = 0;
        for doc in candidates {
            if self.is_empty_on_disk(&doc) {
                self.delete(&doc.id)?;
                reclaimed += 1;
            }
        }
        Ok(reclaimed)
    }

    /// Opens a buffer that originated from an external file, inserting
    /// its row and writing its content to disk in one step.
    ///
    /// FTS indexing is skipped when `doc.size_bytes > THRESHOLD_NORMAL_BYTES`
    /// (large-file and binary tiers).
    pub fn open_from_path(&self, doc: &BufferDocument, content: &str) -> StorageResult<()> {
        let buffer_file = self.buffers_dir.join(&doc.filename);
        std::fs::write(&buffer_file, content)?;

        let tx = self.conn.unchecked_transaction()?;
        queries::insert_buffer(&tx, doc)?;
        if doc.size_bytes <= THRESHOLD_NORMAL_BYTES {
            crate::fts::FtsIndex::new(&tx).insert(&doc.id, &doc.title, content)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Persists content back to the buffer's originating file.
    ///
    /// Both the source file and the mirrored buffer file under
    /// `buffers_dir` are rewritten so Writ's copy remains in sync with
    /// the external file. Fails when the buffer is read-only (binary
    /// hex-view buffers must never write back to their source).
    pub fn save_to_source(&self, id: &str, content: &str) -> StorageResult<()> {
        let doc = queries::get_buffer(&self.conn, id)?;
        if doc.read_only {
            return Err(StorageError::Consistency {
                message: format!("buffer {} is read-only and cannot be saved to source", id),
            });
        }
        let source_path = doc
            .source_path
            .as_ref()
            .ok_or_else(|| StorageError::Consistency {
                message: format!("buffer {} has no source_path", id),
            })?;
        write_atomic(Path::new(source_path), content.as_bytes())?;
        let buffer_file = self.buffers_dir.join(&doc.filename);
        write_atomic(&buffer_file, content.as_bytes())?;

        let tx = self.conn.unchecked_transaction()?;
        queries::update_timestamp(&tx, id)?;
        if doc.size_bytes <= THRESHOLD_NORMAL_BYTES {
            crate::fts::FtsIndex::new(&tx).update(id, &doc.title, content)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Updates the detected or user-assigned language for a buffer.
    pub fn update_language(&self, id: &str, language: Option<&str>) -> StorageResult<()> {
        queries::update_language(&self.conn, id, language)
    }

    /// Drops every FTS row and rebuilds the index from the buffers
    /// table plus on-disk content.
    ///
    /// Intended as a recovery escape hatch when the index drifts from
    /// the buffer set (orphaned rows, missing rows). Currently unwired;
    /// will be exposed as a debug command.
    ///
    /// Large-file and binary buffers (`size_bytes > THRESHOLD_NORMAL_BYTES`)
    /// are excluded, consistent with the write-time skip in
    /// [`Self::save_content`] and [`Self::open_from_path`].
    pub fn rebuild_fts(&self) -> StorageResult<()> {
        self.conn.execute("DELETE FROM buffer_fts", [])?;
        let fts = crate::fts::FtsIndex::new(&self.conn);
        for status in [BufferStatus::Active, BufferStatus::History] {
            let docs = self.list_by_status(status)?;
            for doc in &docs {
                if doc.size_bytes > THRESHOLD_NORMAL_BYTES {
                    continue;
                }
                let file_path = self.buffers_dir.join(&doc.filename);
                let content = if file_path.exists() {
                    std::fs::read_to_string(&file_path).unwrap_or_default()
                } else {
                    String::new()
                };
                fts.insert(&doc.id, &doc.title, &content)?;
            }
        }
        Ok(())
    }

    /// Reconciles the FTS index against the buffer set and rebuilds it on
    /// any drift, returning `true` when a rebuild was performed.
    ///
    /// The transactional writes in [`Self::save_content`] keep the index in
    /// step during normal operation, but a crash between the content-file
    /// write and the commit, or a damaged index file, can still leave the
    /// two out of sync. Run once at boot (audit blocker #53.5): the set of
    /// indexed-eligible buffers (`size_bytes <= THRESHOLD_NORMAL_BYTES`,
    /// either status) must match exactly the set of ids present in the FTS
    /// table; otherwise the whole index is rebuilt from buffers + disk.
    pub fn verify_and_repair_fts(&self) -> StorageResult<bool> {
        let mut expected: std::collections::HashSet<String> = std::collections::HashSet::new();
        for status in [BufferStatus::Active, BufferStatus::History] {
            for doc in self.list_by_status(status)? {
                if doc.size_bytes <= THRESHOLD_NORMAL_BYTES {
                    expected.insert(doc.id);
                }
            }
        }

        let indexed: std::collections::HashSet<String> = {
            let mut stmt = self.conn.prepare(
                "SELECT b.id FROM buffer_fts f JOIN buffers b ON b.rowid = f.rowid",
            )?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            let mut set = std::collections::HashSet::new();
            for row in rows {
                set.insert(row?);
            }
            set
        };

        if expected == indexed {
            return Ok(false);
        }
        warn!(
            expected = expected.len(),
            indexed = indexed.len(),
            "fts index drift detected; rebuilding"
        );
        self.rebuild_fts()?;
        Ok(true)
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

    /// Resolves which active buffers should be restored from the latest
    /// dirty snapshot.
    ///
    /// Reads current `updated_at` timestamps from the database, then
    /// delegates to [`SnapshotManager::recover_buffers`].
    pub fn resolve_recovery(&self) -> StorageResult<Vec<RecoveredBuffer>> {
        let active = self.list_by_status(BufferStatus::Active)?;
        let mut updated_at_map: HashMap<String, String> = HashMap::new();
        for buf in &active {
            updated_at_map.insert(
                buf.id.clone(),
                buf.updated_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            );
        }
        let mgr = SnapshotManager::new(&self.conn);
        mgr.recover_buffers(&updated_at_map)
    }

    /// Collects the current on-disk content for every active buffer.
    ///
    /// Buffers whose content file is missing are silently skipped; the
    /// snapshot will simply contain fewer entries.
    ///
    /// Buffers in the large-file or binary tiers (`size_bytes >
    /// THRESHOLD_NORMAL_BYTES`) are excluded. Reading hundreds of MiB in
    /// the periodic heartbeat would spike RAM and provide little recovery
    /// value (the source file still exists on disk).
    pub fn collect_buffer_contents(&self) -> StorageResult<HashMap<String, String>> {
        let active = self.list_by_status(BufferStatus::Active)?;
        let mut map = HashMap::new();
        for buf in active {
            if buf.size_bytes > THRESHOLD_NORMAL_BYTES {
                continue;
            }
            let path = self.buffers_dir.join(&buf.filename);
            if let Ok(content) = std::fs::read_to_string(&path) {
                map.insert(buf.id, content);
            }
        }
        Ok(map)
    }
}
