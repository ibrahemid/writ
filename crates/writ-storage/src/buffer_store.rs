use std::path::{Path, PathBuf};

use rusqlite::Connection;
use tracing::warn;
use writ_core::buffer::document::{BufferDocument, BufferStatus};

use crate::database::queries;
use crate::errors::{StorageError, StorageResult};

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

    /// Inserts a new buffer row into the database.
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
    /// The `updated_at` column is stamped on every call. FTS update
    /// failures are logged but do not propagate: search may temporarily
    /// trail writes, never block them. Use [`Self::rebuild_fts`] to
    /// recover from a damaged index.
    pub fn save_content(&self, id: &str, content: &str) -> StorageResult<()> {
        let doc = queries::get_buffer(&self.conn, id)?;
        let file_path = self.buffers_dir.join(&doc.filename);
        std::fs::write(&file_path, content)?;
        queries::update_timestamp(&self.conn, id)?;
        let fts = crate::fts::FtsIndex::new(&self.conn);
        if let Err(e) = fts.update(id, &doc.title, content) {
            warn!(buffer_id = id, error = %e, "fts update failed during save_content");
        }
        Ok(())
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
    /// FTS update failures are logged but do not propagate, matching
    /// the policy in [`Self::save_content`]: search may temporarily
    /// trail writes, never block them.
    pub fn rename(&self, id: &str, title: &str) -> StorageResult<()> {
        let doc = queries::get_buffer(&self.conn, id)?;
        queries::rename_buffer(&self.conn, id, title)?;
        let file_path = self.buffers_dir.join(&doc.filename);
        let content = if file_path.exists() {
            std::fs::read_to_string(&file_path).unwrap_or_default()
        } else {
            String::new()
        };
        let fts = crate::fts::FtsIndex::new(&self.conn);
        if let Err(e) = fts.update(id, title, &content) {
            warn!(buffer_id = id, error = %e, "fts update failed during rename");
        }
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

    /// Opens a buffer that originated from an external file, inserting
    /// its row and writing its content to disk in one step.
    pub fn open_from_path(&self, doc: &BufferDocument, content: &str) -> StorageResult<()> {
        queries::insert_buffer(&self.conn, doc)?;
        let buffer_file = self.buffers_dir.join(&doc.filename);
        std::fs::write(&buffer_file, content)?;
        let fts = crate::fts::FtsIndex::new(&self.conn);
        if let Err(e) = fts.update(&doc.id, &doc.title, content) {
            warn!(buffer_id = %doc.id, error = %e, "fts update failed during open_from_path");
        }
        Ok(())
    }

    /// Persists content back to the buffer's originating file.
    ///
    /// Both the source file and the mirrored buffer file under
    /// `buffers_dir` are rewritten so Writ's copy remains in sync with
    /// the external file.
    pub fn save_to_source(&self, id: &str, content: &str) -> StorageResult<()> {
        let doc = queries::get_buffer(&self.conn, id)?;
        let source_path = doc
            .source_path
            .as_ref()
            .ok_or_else(|| StorageError::Consistency {
                message: format!("buffer {} has no source_path", id),
            })?;
        std::fs::write(source_path, content)?;
        let buffer_file = self.buffers_dir.join(&doc.filename);
        std::fs::write(&buffer_file, content)?;
        queries::update_timestamp(&self.conn, id)?;
        let fts = crate::fts::FtsIndex::new(&self.conn);
        if let Err(e) = fts.update(id, &doc.title, content) {
            warn!(buffer_id = id, error = %e, "fts update failed during save_to_source");
        }
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
    pub fn rebuild_fts(&self) -> StorageResult<()> {
        self.conn.execute("DELETE FROM buffer_fts", [])?;
        let fts = crate::fts::FtsIndex::new(&self.conn);
        for status in [BufferStatus::Active, BufferStatus::History] {
            let docs = self.list_by_status(status)?;
            for doc in &docs {
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
}
