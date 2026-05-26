//! Per-buffer preview layout persistence — ADR-009 §"writ-storage".
//!
//! Stores the layout a source-backed buffer was last viewed in, keyed by
//! absolute path. Scratch buffers (no path) are never persisted here — the
//! caller is responsible for not invoking [`LayoutStateStore::set`] for a
//! pathless buffer; the content-type default applies on every open instead.
//!
//! The repository deals in plain string discriminants so the storage layer
//! stays free of the `writ_core::preview::LayoutMode` enum shape. The
//! `src-tauri` adapter maps the enum to and from these strings.

use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension};

use crate::errors::StorageResult;

/// A persisted layout-state row.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutStateRecord {
    /// Absolute source path the row is keyed by.
    pub path: String,
    /// Layout discriminant: `source` | `preview` | `split` | `detached`.
    pub layout_mode: String,
    /// Split ratio (source-pane fraction), present only for `split`.
    pub split_ratio: Option<f32>,
    /// Last active view mode: `source` | `preview`.
    pub last_view_mode: String,
}

/// Repository for the `layout_state` table.
pub struct LayoutStateStore {
    conn: Mutex<Connection>,
}

impl LayoutStateStore {
    /// Wrap an open connection.
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }

    /// Fetch the persisted layout for `path`, if any.
    pub fn get(&self, path: &str) -> StorageResult<Option<LayoutStateRecord>> {
        let conn = self.conn.lock().expect("layout_state mutex poisoned");
        let row = conn
            .query_row(
                "SELECT path, layout_mode, split_ratio, last_view_mode
                 FROM layout_state WHERE path = ?1",
                [path],
                |r| {
                    Ok(LayoutStateRecord {
                        path: r.get(0)?,
                        layout_mode: r.get(1)?,
                        split_ratio: r.get(2)?,
                        last_view_mode: r.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// Upsert the layout for `path`.
    pub fn set(&self, record: &LayoutStateRecord) -> StorageResult<()> {
        let conn = self.conn.lock().expect("layout_state mutex poisoned");
        conn.execute(
            "INSERT INTO layout_state (path, layout_mode, split_ratio, last_view_mode, updated_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now'))
             ON CONFLICT(path) DO UPDATE SET
                 layout_mode = excluded.layout_mode,
                 split_ratio = excluded.split_ratio,
                 last_view_mode = excluded.last_view_mode,
                 updated_at = excluded.updated_at",
            rusqlite::params![
                record.path,
                record.layout_mode,
                record.split_ratio,
                record.last_view_mode,
            ],
        )?;
        Ok(())
    }

    /// Remove the persisted layout for `path` (e.g. on buffer delete).
    pub fn remove(&self, path: &str) -> StorageResult<()> {
        let conn = self.conn.lock().expect("layout_state mutex poisoned");
        conn.execute("DELETE FROM layout_state WHERE path = ?1", [path])?;
        Ok(())
    }
}
