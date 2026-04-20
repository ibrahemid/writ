use rusqlite::{params, Connection};

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
}
