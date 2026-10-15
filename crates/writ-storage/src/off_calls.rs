//! The last program that called while Connected programs was off.
//!
//! `<data dir>/mcp-off-call.json` holds one record: the name the program sent
//! and when. The server writes it when it answers that it is off, at most once
//! per process; the app watches the file and offers to turn Connected programs
//! on, once (ADR-031 rule 7.2). The record carries no path, tool or text.

use std::path::{Path, PathBuf};

use writ_core::activity::{ClientId, OffCall};

use crate::errors::StorageResult;

/// The file inside the data folder.
const FILE: &str = "mcp-off-call.json";

/// Where the file sits inside `dir`.
pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// The record, or `None` for a file that is not there or does not parse.
pub fn read(dir: &Path) -> Option<OffCall> {
    let bytes = std::fs::read(path(dir)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Records that `client` called while the server was off.
pub fn note(dir: &Path, client: &ClientId) -> StorageResult<()> {
    note_at(dir, client, chrono::Utc::now())
}

/// [`note`] against a stated clock.
pub fn note_at(
    dir: &Path,
    client: &ClientId,
    now: chrono::DateTime<chrono::Utc>,
) -> StorageResult<()> {
    let record = OffCall {
        name: client.name.clone(),
        at: now,
    };
    let bytes = serde_json::to_vec_pretty(&record)?;
    crate::atomic::write_atomic(&path(dir), &bytes)?;
    Ok(())
}
