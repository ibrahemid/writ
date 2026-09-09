//! Programs the harness has seen and the user has not decided on.
//!
//! `<data dir>/mcp-pending.json` holds one entry per client name. It exists
//! because the activity log cannot answer "who is waiting": the log is capped
//! and rotates, so a program looping calls the gate is refusing writes its own
//! rows over the pending ones the user needed in order to decide about it
//! (ADR-031 rule 5.4 caps the log; rule 7.2 wants the client shown). One entry
//! per name is unevictable by volume.
//!
//! Written through [`crate::atomic::write_atomic`], under the same lock the
//! activity log takes, so a reader never sees half a file and two servers never
//! interleave.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use writ_core::activity::{ClientId, PendingClient};

use crate::activity_log;
use crate::errors::StorageResult;

/// The file inside the data folder.
const FILE: &str = "mcp-pending.json";

/// The shape on disk. An object rather than a bare array, so a later field can
/// be added without every older build failing to parse the file.
#[derive(Debug, Default, Serialize, Deserialize)]
struct PendingFile {
    #[serde(default)]
    clients: Vec<PendingClient>,
}

/// Where the file sits inside `dir`.
pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// Everyone waiting, oldest first seen first.
///
/// A file that is not there, or one that does not parse, reads as nobody
/// waiting: this is a convenience for the settings surface, and a bad file is
/// not a reason to fail a call.
pub fn read(dir: &Path) -> Vec<PendingClient> {
    let _guard = activity_log::open_lock(dir)
        .ok()
        .and_then(|file| file.lock_shared().ok().map(|()| file));
    let mut clients = read_unlocked(dir).clients;
    clients.sort_by_key(|entry| entry.first_seen);
    clients
}

/// Records `calls` more calls from `client`, creating the entry on first sight.
///
/// The caller decides how often this is worth doing; rate limiting lives in the
/// gate, which is what knows how many calls it has not written yet.
pub fn note_calls(dir: &Path, client: &ClientId, calls: u64) -> StorageResult<()> {
    note_calls_at(dir, client, calls, chrono::Utc::now())
}

/// [`note_calls`] against a stated clock, so a test can place the entries in
/// time rather than race one.
pub fn note_calls_at(
    dir: &Path,
    client: &ClientId,
    calls: u64,
    now: chrono::DateTime<chrono::Utc>,
) -> StorageResult<()> {
    with_file(dir, |file| {
        match file
            .clients
            .iter_mut()
            .find(|entry| entry.name == client.name)
        {
            Some(entry) => {
                entry.calls = entry.calls.saturating_add(calls);
                entry.last_seen = now;
                if client.version.is_some() {
                    entry.version = client.version.clone();
                }
            }
            None => {
                let mut entry = PendingClient::first_call(client, now);
                entry.calls = calls.max(1);
                file.clients.push(entry);
            }
        }
    })
}

/// Drops the entry for `name`. A name that is not there is already forgotten.
pub fn forget(dir: &Path, name: &str) -> StorageResult<()> {
    with_file(dir, |file| {
        file.clients.retain(|entry| entry.name != name);
    })
}

/// Reads, applies `change`, and writes the result back, all under the exclusive
/// lock so two servers cannot lose each other's edit.
fn with_file(dir: &Path, change: impl FnOnce(&mut PendingFile)) -> StorageResult<()> {
    let lock = activity_log::open_lock(dir)?;
    lock.lock()?;
    let mut file = read_unlocked(dir);
    change(&mut file);
    let written = write_unlocked(dir, &file);
    let _ = lock.unlock();
    written
}

fn read_unlocked(dir: &Path) -> PendingFile {
    let Ok(text) = std::fs::read_to_string(path(dir)) else {
        return PendingFile::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn write_unlocked(dir: &Path, file: &PendingFile) -> StorageResult<()> {
    let text = serde_json::to_vec_pretty(file)?;
    crate::atomic::write_atomic(&path(dir), &text)?;
    Ok(())
}
