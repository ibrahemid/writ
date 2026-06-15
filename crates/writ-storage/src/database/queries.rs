use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use writ_core::buffer::document::{BufferDocument, BufferStatus};

use crate::errors::{StorageError, StorageResult};

fn parse_rfc3339(s: &str) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| rusqlite::Error::InvalidColumnName(format!("invalid datetime: {}", s)))
}

/// Maps a buffers row into a [`BufferDocument`], reading every field by
/// column name rather than position.
///
/// By-name access (audit blocker #53.8) decouples the mapper from the
/// order columns appear in a `SELECT`: a future migration that adds or
/// reorders columns can no longer silently shift a value into the wrong
/// field. Every `SELECT` feeding this mapper must therefore alias the
/// columns it needs by their schema names.
fn row_to_document(row: &rusqlite::Row) -> rusqlite::Result<BufferDocument> {
    let status_str: String = row.get("status")?;
    let status = match status_str.as_str() {
        "active" => BufferStatus::Active,
        "history" => BufferStatus::History,
        _ => return Err(rusqlite::Error::InvalidColumnName(status_str)),
    };

    let created_str: String = row.get("created_at")?;
    let updated_str: String = row.get("updated_at")?;
    let closed_str: Option<String> = row.get("closed_at")?;

    let created_at = parse_rfc3339(&created_str)?;
    let updated_at = parse_rfc3339(&updated_str)?;
    let closed_at = closed_str.map(|s| parse_rfc3339(&s)).transpose()?;

    let cursor_pos: i64 = row.get("cursor_pos")?;
    let scroll_pos: i64 = row.get("scroll_pos")?;
    let tab_order: i64 = row.get("tab_order")?;
    let read_only: i64 = row.get("read_only")?;
    let size_bytes: i64 = row.get("size_bytes")?;

    Ok(BufferDocument {
        id: row.get("id")?,
        title: row.get("title")?,
        filename: row.get("filename")?,
        status,
        language: row.get("language")?,
        source_path: row.get("source_path")?,
        cursor_pos: cursor_pos as u64,
        scroll_pos: scroll_pos as u64,
        tab_order: tab_order as u32,
        created_at,
        updated_at,
        closed_at,
        read_only: read_only != 0,
        size_bytes: size_bytes as u64,
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
             tab_order, created_at, updated_at, closed_at, read_only, size_bytes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
            doc.read_only as i64,
            doc.size_bytes as i64,
        ],
    )?;
    Ok(())
}

/// Fetches a buffer row by id, mapping a missing row to
/// [`StorageError::Consistency`].
pub fn get_buffer(conn: &Connection, id: &str) -> StorageResult<BufferDocument> {
    let mut stmt = conn.prepare(
        "SELECT id, title, filename, status, language, source_path,
                cursor_pos, scroll_pos, tab_order, created_at, updated_at, closed_at,
                read_only, size_bytes
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
                cursor_pos, scroll_pos, tab_order, created_at, updated_at, closed_at,
                read_only, size_bytes
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
                cursor_pos, scroll_pos, tab_order, created_at, updated_at, closed_at,
                read_only, size_bytes
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
                cursor_pos, scroll_pos, tab_order, created_at, updated_at, closed_at,
                read_only, size_bytes
         FROM buffers WHERE source_path = ?1 AND status = 'history'
         ORDER BY updated_at DESC LIMIT 1",
    )?;
    let result = stmt
        .query_row(params![source_path], row_to_document)
        .optional()
        .map_err(StorageError::Database)?;
    Ok(result)
}

/// Returns every never-renamed scratch buffer, regardless of status,
/// ordered by tab position. Callers filter on disk emptiness and status.
///
/// A buffer is considered never-renamed when `source_path IS NULL` and
/// either (a) `title = filename` (legacy rows minted before filename was
/// decoupled from title) or (b) `title` matches the default scratch
/// pattern `writ-<digits>` produced by `BufferManager::create_buffer`
/// when no title is supplied.
pub fn list_scratch_candidates(conn: &Connection) -> StorageResult<Vec<BufferDocument>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, filename, status, language, source_path,
                cursor_pos, scroll_pos, tab_order, created_at, updated_at, closed_at,
                read_only, size_bytes
         FROM buffers
         WHERE source_path IS NULL
           AND (title = filename OR title GLOB 'writ-[0-9]*')
         ORDER BY tab_order ASC",
    )?;
    let rows = stmt.query_map([], row_to_document)?;
    let mut docs = Vec::new();
    for row in rows {
        docs.push(row?);
    }
    Ok(docs)
}

/// Updates the detected or user-assigned language for a buffer.
pub fn update_language(conn: &Connection, id: &str, language: Option<&str>) -> StorageResult<()> {
    conn.execute(
        "UPDATE buffers SET language = ?1, updated_at = ?2 WHERE id = ?3",
        params![language, Utc::now().to_rfc3339(), id],
    )?;
    Ok(())
}

/// Rewrites a buffer's on-disk mirror `filename` without touching any
/// other field, including `updated_at` (filename normalization is an
/// internal repair, not a user edit).
pub fn update_filename(conn: &Connection, id: &str, filename: &str) -> StorageResult<()> {
    conn.execute(
        "UPDATE buffers SET filename = ?1 WHERE id = ?2",
        params![filename, id],
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::migrations::run_migrations;
    use writ_core::buffer::document::BufferDocument;

    fn doc_fixture() -> BufferDocument {
        let now = Utc::now();
        BufferDocument {
            id: "buf-1".to_string(),
            title: "Title".to_string(),
            filename: "buf-1.txt".to_string(),
            status: BufferStatus::History,
            language: Some("rust".to_string()),
            source_path: Some("/tmp/x.rs".to_string()),
            cursor_pos: 7,
            scroll_pos: 42,
            tab_order: 3,
            created_at: now,
            updated_at: now,
            closed_at: Some(now),
            read_only: true,
            size_bytes: 99,
        }
    }

    #[test]
    fn row_to_document_reads_by_name_not_position() {
        // Audit blocker #53.8: row_to_document must bind fields by column
        // name so a SELECT whose columns are in a different order than the
        // table definition still maps every value to the correct field. A
        // deliberately scrambled column order would silently corrupt
        // positional access; by-name access survives it.
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        let doc = doc_fixture();
        insert_buffer(&conn, &doc).unwrap();

        let scrambled = conn
            .query_row(
                "SELECT size_bytes, closed_at, id, read_only, language, status,
                        scroll_pos, title, source_path, updated_at, filename,
                        cursor_pos, created_at, tab_order
                 FROM buffers WHERE id = ?1",
                params![doc.id],
                row_to_document,
            )
            .unwrap();

        assert_eq!(scrambled.id, doc.id);
        assert_eq!(scrambled.title, doc.title);
        assert_eq!(scrambled.filename, doc.filename);
        assert_eq!(scrambled.status, BufferStatus::History);
        assert_eq!(scrambled.language.as_deref(), Some("rust"));
        assert_eq!(scrambled.source_path.as_deref(), Some("/tmp/x.rs"));
        assert_eq!(scrambled.cursor_pos, 7);
        assert_eq!(scrambled.scroll_pos, 42);
        assert_eq!(scrambled.tab_order, 3);
        assert!(scrambled.read_only);
        assert_eq!(scrambled.size_bytes, 99);
        assert!(scrambled.closed_at.is_some());
    }
}
