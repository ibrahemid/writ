use std::collections::HashMap;

use chrono::Utc;
use uuid::Uuid;

use crate::buffer::document::{BufferDocument, BufferStatus};
use crate::errors::{WritError, WritResult};

pub struct BufferManager {
    buffers: HashMap<String, BufferDocument>,
    next_tab_order: u32,
}

impl BufferManager {
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
            next_tab_order: 0,
        }
    }

    pub fn create_buffer(&mut self, title: Option<String>) -> WritResult<BufferDocument> {
        let now = Utc::now();
        let id = Uuid::new_v4().to_string();
        let resolved_title = title.unwrap_or_else(|| format!("writ-{}", now.timestamp_millis()));
        let tab_order = self.next_tab_order;
        self.next_tab_order += 1;

        let doc = BufferDocument {
            id: id.clone(),
            title: resolved_title.clone(),
            filename: resolved_title,
            status: BufferStatus::Active,
            language: None,
            source_path: None,
            cursor_pos: 0,
            scroll_pos: 0,
            tab_order,
            created_at: now,
            updated_at: now,
            closed_at: None,
        };

        self.buffers.insert(id, doc.clone());
        Ok(doc)
    }

    pub fn open_external(&mut self, path: String) -> WritResult<BufferDocument> {
        let filename = std::path::Path::new(&path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&path)
            .to_string();

        let now = Utc::now();
        let id = Uuid::new_v4().to_string();
        let tab_order = self.next_tab_order;
        self.next_tab_order += 1;

        let doc = BufferDocument {
            id: id.clone(),
            title: filename.clone(),
            filename,
            status: BufferStatus::Active,
            language: None,
            source_path: Some(path),
            cursor_pos: 0,
            scroll_pos: 0,
            tab_order,
            created_at: now,
            updated_at: now,
            closed_at: None,
        };

        self.buffers.insert(id, doc.clone());
        Ok(doc)
    }

    pub fn get_buffer(&self, id: &str) -> WritResult<&BufferDocument> {
        self.buffers
            .get(id)
            .ok_or_else(|| WritError::BufferNotFound { id: id.to_string() })
    }

    pub fn close_buffer(&mut self, id: &str) -> WritResult<()> {
        let doc = self
            .buffers
            .get_mut(id)
            .ok_or_else(|| WritError::BufferNotFound { id: id.to_string() })?;

        doc.status = BufferStatus::History;
        doc.closed_at = Some(Utc::now());
        doc.updated_at = Utc::now();
        Ok(())
    }

    pub fn restore_buffer(&mut self, id: &str) -> WritResult<()> {
        let doc = self
            .buffers
            .get_mut(id)
            .ok_or_else(|| WritError::BufferNotFound { id: id.to_string() })?;

        doc.status = BufferStatus::Active;
        doc.closed_at = None;
        doc.updated_at = Utc::now();
        Ok(())
    }

    pub fn delete_buffer(&mut self, id: &str) -> WritResult<BufferDocument> {
        self.buffers
            .remove(id)
            .ok_or_else(|| WritError::BufferNotFound { id: id.to_string() })
    }

    pub fn list_active(&self) -> Vec<&BufferDocument> {
        let mut active: Vec<&BufferDocument> = self
            .buffers
            .values()
            .filter(|doc| doc.status == BufferStatus::Active)
            .collect();
        active.sort_by_key(|doc| doc.tab_order);
        active
    }

    pub fn list_history(&self) -> Vec<&BufferDocument> {
        let mut history: Vec<&BufferDocument> = self
            .buffers
            .values()
            .filter(|doc| doc.status == BufferStatus::History)
            .collect();
        history.sort_by(|a, b| b.closed_at.cmp(&a.closed_at));
        history
    }

    pub fn reorder_tabs(&mut self, ordered_ids: &[String]) -> WritResult<()> {
        for (position, id) in ordered_ids.iter().enumerate() {
            let doc = self
                .buffers
                .get_mut(id.as_str())
                .ok_or_else(|| WritError::BufferNotFound { id: id.clone() })?;
            doc.tab_order = position as u32;
            doc.updated_at = Utc::now();
        }
        Ok(())
    }
}

impl Default for BufferManager {
    fn default() -> Self {
        Self::new()
    }
}
