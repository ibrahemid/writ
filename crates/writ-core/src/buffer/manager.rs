use std::collections::HashMap;
use std::path::{Component, Path};
use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::buffer::document::{BufferDocument, BufferStatus};
use crate::errors::{WritError, WritResult};
use crate::events::bus::{EventBus, WritEvent};

fn validate_buffer_title(title: &str) -> WritResult<()> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(WritError::InvalidTitle {
            reason: "title must not be empty or whitespace-only".to_string(),
        });
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err(WritError::InvalidTitle {
            reason: format!("title must not contain path separators: {title:?}"),
        });
    }
    if trimmed.contains('\0') {
        return Err(WritError::InvalidTitle {
            reason: "title must not contain NUL bytes".to_string(),
        });
    }

    let mut components = Path::new(trimmed).components();
    let first = components.next().ok_or_else(|| WritError::InvalidTitle {
        reason: format!("title yields no path components: {title:?}"),
    })?;
    if components.next().is_some() {
        return Err(WritError::InvalidTitle {
            reason: format!("title must be a single path component: {title:?}"),
        });
    }
    match first {
        Component::Normal(_) => Ok(()),
        Component::ParentDir | Component::CurDir | Component::RootDir | Component::Prefix(_) => {
            Err(WritError::InvalidTitle {
                reason: format!("title must be a single Normal component: {title:?}"),
            })
        }
    }
}

/// In-memory collection of buffers with lifecycle operations.
///
/// `BufferManager` is the authoritative view of which buffers exist and
/// what state each is in. Persistence is performed separately by
/// `writ-storage`; this type owns only in-memory state.
///
/// When constructed with [`Self::with_event_bus`], buffer lifecycle
/// transitions are published as [`WritEvent`] payloads onto the bus.
/// A manager without a bus is silent — useful for tests and for any
/// caller that does not care about lifecycle events.
pub struct BufferManager {
    buffers: HashMap<String, BufferDocument>,
    next_tab_order: u32,
    event_bus: Option<Arc<EventBus>>,
}

impl BufferManager {
    /// Creates a new, empty manager with no event bus.
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
            next_tab_order: 0,
            event_bus: None,
        }
    }

    /// Attaches an event bus so lifecycle transitions are published.
    pub fn with_event_bus(mut self, bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    fn publish(&self, event: WritEvent) {
        if let Some(bus) = &self.event_bus {
            bus.emit(event);
        }
    }

    /// Creates a new scratch buffer and assigns the next tab order.
    ///
    /// When `title` is `None`, a timestamp-derived title (`writ-<ms>`) is
    /// generated so that buffers created in a tight loop remain distinct.
    pub fn create_buffer(&mut self, title: Option<String>) -> WritResult<BufferDocument> {
        if let Some(ref t) = title {
            validate_buffer_title(t)?;
        }

        let now = Utc::now();
        let id = Uuid::new_v4().to_string();
        let resolved_title = title
            .map(|t| t.trim().to_string())
            .unwrap_or_else(|| format!("writ-{}", now.timestamp_millis()));
        let filename = format!("{id}.txt");
        let tab_order = self.next_tab_order;
        self.next_tab_order += 1;

        let doc = BufferDocument {
            id: id.clone(),
            title: resolved_title,
            filename,
            status: BufferStatus::Active,
            language: None,
            source_path: None,
            cursor_pos: 0,
            scroll_pos: 0,
            tab_order,
            created_at: now,
            updated_at: now,
            closed_at: None,
            read_only: false,
            size_bytes: 0,
        };

        self.buffers.insert(id, doc.clone());
        self.publish(WritEvent::BufferOpened {
            id: doc.id.clone(),
            title: doc.title.clone(),
        });
        Ok(doc)
    }

    /// Opens an external file as a new active buffer.
    ///
    /// The buffer's `source_path` is set to `path` so later saves write
    /// back to the origin file. The title is derived from the file name.
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
            read_only: false,
            size_bytes: 0,
        };

        self.buffers.insert(id, doc.clone());
        self.publish(WritEvent::BufferOpened {
            id: doc.id.clone(),
            title: doc.title.clone(),
        });
        Ok(doc)
    }

    /// Returns the buffer with the given id, or
    /// [`WritError::BufferNotFound`] if none exists.
    pub fn get_buffer(&self, id: &str) -> WritResult<&BufferDocument> {
        self.buffers
            .get(id)
            .ok_or_else(|| WritError::BufferNotFound { id: id.to_string() })
    }

    /// Moves a buffer to [`BufferStatus::History`] and stamps `closed_at`.
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

    /// Restores a history buffer back to [`BufferStatus::Active`].
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

    /// Permanently removes a buffer from the manager and returns it.
    pub fn delete_buffer(&mut self, id: &str) -> WritResult<BufferDocument> {
        self.buffers
            .remove(id)
            .ok_or_else(|| WritError::BufferNotFound { id: id.to_string() })
    }

    /// Returns all active buffers, ordered by their tab position.
    pub fn list_active(&self) -> Vec<&BufferDocument> {
        let mut active: Vec<&BufferDocument> = self
            .buffers
            .values()
            .filter(|doc| doc.status == BufferStatus::Active)
            .collect();
        active.sort_by_key(|doc| doc.tab_order);
        active
    }

    /// Returns all history buffers, most recently closed first.
    pub fn list_history(&self) -> Vec<&BufferDocument> {
        let mut history: Vec<&BufferDocument> = self
            .buffers
            .values()
            .filter(|doc| doc.status == BufferStatus::History)
            .collect();
        history.sort_by_key(|doc| std::cmp::Reverse(doc.closed_at));
        history
    }

    /// Rewrites tab order to match `ordered_ids`.
    ///
    /// Returns [`WritError::BufferNotFound`] if any id is unknown; partial
    /// reorderings may be observed in that case.
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
