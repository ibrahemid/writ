use rusqlite::{params, Connection};

use crate::errors::StorageResult;

pub struct FtsIndex<'a> {
    conn: &'a Connection,
}

impl<'a> FtsIndex<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn insert(&self, buffer_id: &str, title: &str, content: &str) -> StorageResult<()> {
        self.conn.execute(
            "INSERT INTO buffer_fts(rowid, title, content)
             SELECT rowid, ?2, ?3 FROM buffers WHERE id = ?1",
            params![buffer_id, title, content],
        )?;
        Ok(())
    }

    pub fn delete(&self, buffer_id: &str) -> StorageResult<()> {
        self.conn.execute(
            "DELETE FROM buffer_fts WHERE rowid = (SELECT rowid FROM buffers WHERE id = ?1)",
            params![buffer_id],
        )?;
        Ok(())
    }

    pub fn update(&self, buffer_id: &str, title: &str, content: &str) -> StorageResult<()> {
        self.delete(buffer_id)?;
        self.insert(buffer_id, title, content)?;
        Ok(())
    }

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
