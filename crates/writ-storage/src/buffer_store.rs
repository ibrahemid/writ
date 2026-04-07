use std::path::{Path, PathBuf};

use rusqlite::Connection;
use writ_core::buffer::document::{BufferDocument, BufferStatus};

use crate::database::queries;
use crate::errors::{StorageError, StorageResult};

pub struct BufferStore {
    conn: Connection,
    buffers_dir: PathBuf,
}

impl BufferStore {
    pub fn new(conn: Connection, buffers_dir: PathBuf) -> Self {
        Self { conn, buffers_dir }
    }

    pub fn buffers_dir(&self) -> &Path {
        &self.buffers_dir
    }

    pub fn insert(&self, doc: &BufferDocument) -> StorageResult<()> {
        queries::insert_buffer(&self.conn, doc)
    }

    pub fn get(&self, id: &str) -> StorageResult<BufferDocument> {
        queries::get_buffer(&self.conn, id)
    }

    pub fn close(&self, id: &str) -> StorageResult<()> {
        queries::close_buffer(&self.conn, id)
    }

    pub fn restore(&self, id: &str) -> StorageResult<()> {
        queries::restore_buffer(&self.conn, id)
    }

    pub fn delete(&self, id: &str) -> StorageResult<()> {
        let doc = queries::get_buffer(&self.conn, id)?;
        let file_path = self.buffers_dir.join(&doc.filename);
        if file_path.exists() {
            std::fs::remove_file(&file_path)?;
        }
        queries::delete_buffer(&self.conn, id)
    }

    pub fn list_by_status(&self, status: BufferStatus) -> StorageResult<Vec<BufferDocument>> {
        let status_str = match status {
            BufferStatus::Active => "active",
            BufferStatus::History => "history",
        };
        queries::list_buffers_by_status(&self.conn, status_str)
    }

    pub fn save_content(&self, id: &str, content: &str) -> StorageResult<()> {
        let doc = queries::get_buffer(&self.conn, id)?;
        let file_path = self.buffers_dir.join(&doc.filename);
        std::fs::write(&file_path, content)?;
        queries::update_timestamp(&self.conn, id)?;
        let fts = crate::fts::FtsIndex::new(&self.conn);
        let _ = fts.update(id, &doc.title, content);
        Ok(())
    }

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

    pub fn rename(&self, id: &str, title: &str) -> StorageResult<()> {
        queries::rename_buffer(&self.conn, id, title)
    }

    pub fn update_tab_order(&self, id: &str, order: u32) -> StorageResult<()> {
        queries::update_tab_order(&self.conn, id, order)
    }

    pub fn search(&self, query: &str) -> StorageResult<Vec<String>> {
        let fts = crate::fts::FtsIndex::new(&self.conn);
        fts.search(query)
    }
}
