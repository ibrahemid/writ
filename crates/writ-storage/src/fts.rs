use rusqlite::{params, Connection};
use writ_core::search::{build_hit, SearchHit};

use crate::errors::StorageResult;

/// FTS5 index over buffer titles and content.
///
/// The index lives in a virtual table (`buffer_fts`) that joins on
/// `buffers.rowid`. Writes go through [`FtsIndex::update`] so updates
/// remain atomic at the row level.
pub struct FtsIndex<'a> {
    conn: &'a Connection,
}

impl<'a> FtsIndex<'a> {
    /// Constructs an index view over the given connection.
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Inserts a new row into the FTS index for the given buffer.
    pub fn insert(&self, buffer_id: &str, title: &str, content: &str) -> StorageResult<()> {
        self.conn.execute(
            "INSERT INTO buffer_fts(rowid, title, content)
             SELECT rowid, ?2, ?3 FROM buffers WHERE id = ?1",
            params![buffer_id, title, content],
        )?;
        Ok(())
    }

    /// Removes the FTS row associated with the given buffer, if any.
    pub fn delete(&self, buffer_id: &str) -> StorageResult<()> {
        self.conn.execute(
            "DELETE FROM buffer_fts WHERE rowid = (SELECT rowid FROM buffers WHERE id = ?1)",
            params![buffer_id],
        )?;
        Ok(())
    }

    /// Replaces the FTS row for a buffer with a fresh one.
    pub fn update(&self, buffer_id: &str, title: &str, content: &str) -> StorageResult<()> {
        self.delete(buffer_id)?;
        self.insert(buffer_id, title, content)?;
        Ok(())
    }

    /// Runs an FTS5 `MATCH` query and returns buffer ids sorted by rank.
    pub fn search(&self, query: &str) -> StorageResult<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT b.id FROM buffer_fts f
             JOIN buffers b ON b.rowid = f.rowid
             WHERE buffer_fts MATCH ?1
             ORDER BY rank",
        )?;
        let ids = stmt
            .query_map(params![query], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    /// Returns the total number of buffers matching `query`, independent of any
    /// result-limit, so the UI can report "N of M" honestly.
    pub fn count(&self, query: &str) -> StorageResult<usize> {
        let total: i64 = self.conn.query_row(
            "SELECT count(*) FROM buffer_fts WHERE buffer_fts MATCH ?1",
            params![query],
            |row| row.get(0),
        )?;
        Ok(total as usize)
    }

    /// Runs an FTS5 `MATCH` query and returns up to `limit` display hits sorted
    /// by rank. Each hit carries the buffer title, the matching line number, and
    /// a highlighted snippet built by [`build_hit`] from the indexed content.
    pub fn search_hits(
        &self,
        query: &str,
        terms: &[String],
        limit: usize,
    ) -> StorageResult<Vec<SearchHit>> {
        let mut stmt = self.conn.prepare(
            "SELECT b.id, f.title, f.content FROM buffer_fts f
             JOIN buffers b ON b.rowid = f.rowid
             WHERE buffer_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut hits = Vec::new();
        for row in rows {
            let (id, title, content) = row?;
            hits.push(build_hit(&id, &title, &content, terms));
        }
        Ok(hits)
    }
}
