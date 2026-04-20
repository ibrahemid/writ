use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use writ_core::buffer::document::{BufferDocument, BufferStatus};

use crate::errors::{StorageError, StorageResult};

fn parse_rfc3339(s: &str) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| rusqlite::Error::InvalidColumnName(format!("invalid datetime: {}", s)))
}

fn row_to_document(row: &rusqlite::Row) -> rusqlite::Result<BufferDocument> {
    let status_str: String = row.get(3)?;
    let status = match status_str.as_str() {
        "active" => BufferStatus::Active,
        "history" => BufferStatus::History,
        _ => return Err(rusqlite::Error::InvalidColumnName(status_str)),
    };

    let created_str: String = row.get(9)?;
    let updated_str: String = row.get(10)?;
    let closed_str: Option<String> = row.get(11)?;

    let created_at = parse_rfc3339(&created_str)?;
    let updated_at = parse_rfc3339(&updated_str)?;
    let closed_at = closed_str.map(|s| parse_rfc3339(&s)).transpose()?;

    let cursor_pos: i64 = row.get(6)?;
    let scroll_pos: i64 = row.get(7)?;
    let tab_order: i64 = row.get(8)?;

    Ok(BufferDocument {
        id: row.get(0)?,
        title: row.get(1)?,
        filename: row.get(2)?,
        status,
        language: row.get(4)?,
        source_path: row.get(5)?,
        cursor_pos: cursor_pos as u64,
        scroll_pos: scroll_pos as u64,
        tab_order: tab_order as u32,
        created_at,
        updated_at,
        closed_at,
    })
}

/// Inserts a new buffer row.
pub fn insert_buffer(conn: &Connection, doc: &BufferDocument) -> StorageResult<()> {
    let status = match doc.status {
        BufferStatus::Active => "active",
        BufferStatus::History => "history",
    };
    let closed_at = doc.closed_at.map(|dt| dt.to_rfc3339());
    conn.execute(
        "INSERT INTO buffers
            (id, title, filename, status, language, source_path, cursor_pos, scroll_pos,
             tab_order, created_at, updated_at, closed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            doc.id,
            doc.title,
            doc.filename,
            status,
            doc.language,
            doc.source_path,
            doc.cursor_pos as i64,
            doc.scroll_pos as i64,
            doc.tab_order as i64,
            doc.created_at.to_rfc3339(),
            doc.updated_at.to_rfc3339(),
            closed_at,
        ],
    )?;
    Ok(())
}

/// Fetches a buffer row by id, mapping a missing row to
/// [`StorageError::Consistency`].
pub fn get_buffer(conn: &Connection, id: &str) -> StorageResult<BufferDocument> {
    let mut stmt = conn.prepare(
        "SELECT id, title, filename, status, language, source_path,
                cursor_pos, scroll_pos, tab_order, created_at, updated_at, closed_at
         FROM buffers WHERE id = ?1",
    )?;
    let result = stmt
        .query_row(params![id], row_to_document)
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StorageError::Consistency {
                message: format!("buffer not found: {}", id),
            },
            other => StorageError::Database(other),
        })?;
    Ok(result)
}

/// Lists buffers filtered by status literal (`"active"` or `"history"`).
pub fn list_buffers_by_status(
    conn: &Connection,
    status: &str,
) -> StorageResult<Vec<BufferDocument>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, filename, status, language, source_path,
                cursor_pos, scroll_pos, tab_order, created_at, updated_at, closed_at
         FROM buffers WHERE status = ?1 ORDER BY tab_order ASC",
    )?;
    let rows = stmt.query_map(params![status], row_to_document)?;
    let mut docs = Vec::new();
    for row in rows {
        docs.push(row?);
    }
    Ok(docs)
}

/// Marks the buffer as history and stamps `closed_at`.
pub fn close_buffer(conn: &Connection, id: &str) -> StorageResult<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE buffers SET status = 'history', closed_at = ?1, updated_at = ?2 WHERE id = ?3",
        params![now.clone(), now, id],
    )?;
    Ok(())
}

/// Restores a history buffer to active state and clears `closed_at`.
pub fn restore_buffer(conn: &Connection, id: &str) -> StorageResult<()> {
    conn.execute(
        "UPDATE buffers SET status = 'active', closed_at = NULL, updated_at = ?1 WHERE id = ?2",
        params![Utc::now().to_rfc3339(), id],
    )?;
    Ok(())
}

/// Deletes a buffer row by id.
pub fn delete_buffer(conn: &Connection, id: &str) -> StorageResult<()> {
    conn.execute("DELETE FROM buffers WHERE id = ?1", params![id])?;
    Ok(())
}

/// Updates the persistent tab order for a buffer.
pub fn update_tab_order(conn: &Connection, id: &str, order: u32) -> StorageResult<()> {
    conn.execute(
        "UPDATE buffers SET tab_order = ?1 WHERE id = ?2",
        params![order as i64, id],
    )?;
    Ok(())
}

/// Renames a buffer's title and stamps `updated_at`.
pub fn rename_buffer(conn: &Connection, id: &str, title: &str) -> StorageResult<()> {
    conn.execute(
        "UPDATE buffers SET title = ?1, updated_at = ?2 WHERE id = ?3",
        params![title, Utc::now().to_rfc3339(), id],
    )?;
    Ok(())
}

/// Returns the active buffer with the given `source_path`, if any.
pub fn find_active_by_source_path(
    conn: &Connection,
    source_path: &str,
) -> StorageResult<Option<BufferDocument>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, filename, status, language, source_path,
                cursor_pos, scroll_pos, tab_order, created_at, updated_at, closed_at
         FROM buffers WHERE source_path = ?1 AND status = 'active' LIMIT 1",
    )?;
    let result = stmt
        .query_row(params![source_path], row_to_document)
        .optional()
        .map_err(StorageError::Database)?;
    Ok(result)
}

/// Returns the most recently updated history buffer with the given
/// `source_path`, if any.
pub fn find_history_by_source_path(
    conn: &Connection,
    source_path: &str,
) -> StorageResult<Option<BufferDocument>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, filename, status, language, source_path,
                cursor_pos, scroll_pos, tab_order, created_at, updated_at, closed_at
         FROM buffers WHERE source_path = ?1 AND status = 'history'
         ORDER BY updated_at DESC LIMIT 1",
    )?;
    let result = stmt
        .query_row(params![source_path], row_to_document)
        .optional()
        .map_err(StorageError::Database)?;
    Ok(result)
}

/// Updates the detected or user-assigned language for a buffer.
pub fn update_language(conn: &Connection, id: &str, language: Option<&str>) -> StorageResult<()> {
    conn.execute(
        "UPDATE buffers SET language = ?1, updated_at = ?2 WHERE id = ?3",
        params![language, Utc::now().to_rfc3339(), id],
    )?;
    Ok(())
}

/// Stamps a buffer's `updated_at` to now without changing any other
/// fields.
pub fn update_timestamp(conn: &Connection, id: &str) -> StorageResult<()> {
    conn.execute(
        "UPDATE buffers SET updated_at = ?1 WHERE id = ?2",
        params![Utc::now().to_rfc3339(), id],
    )?;
    Ok(())
}
