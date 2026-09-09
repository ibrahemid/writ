//! The activity log: append-only JSONL in Writ's data folder.
//!
//! One [`ActivityRecord`] per line at `<data dir>/activity.jsonl`. Two
//! processes write it — the app, and the `writ mcp` server a client launched —
//! so every line is one `write_all` on a handle opened with `O_APPEND`, which
//! the kernel does not split against another appender. There is no lock, and
//! the log never touches `writ.db`: a second process does not share a SQLite
//! file (ADR-031 rule 5.4).
//!
//! It is capped rather than trimmed: at [`ROTATE_AT_BYTES`] the current file
//! becomes `activity.1.jsonl` and a new one starts, and only that one
//! generation is kept. Rotation is the one step `O_APPEND` does not make safe
//! on its own — two processes reaching the cap together would both rename, and
//! the loser would either error or drop the generation the winner had just
//! filled. So every append holds a shared advisory lock on `activity.lock` and
//! a rotation upgrades to the exclusive one, re-checking the size before it
//! renames. `File::lock` and its shared partner have been in std since 1.89,
//! which is this workspace's minimum, so the lock costs no dependency. A line that does not parse is skipped, never fatal —
//! the one thing a torn write from an older build could leave behind must not
//! take the panel down with it.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use writ_core::activity::ActivityRecord;

use crate::errors::StorageResult;

/// Size at which the current file is rotated, in bytes (ADR-031 rule 5.4).
pub const ROTATE_AT_BYTES: u64 = 5 * 1024 * 1024;

/// The file being appended to.
const CURRENT: &str = "activity.jsonl";

/// The one generation kept behind it.
const PREVIOUS: &str = "activity.1.jsonl";

/// The sidecar every writer locks. It carries no content: it exists so that a
/// rotation can hold something the appenders also hold, which the log files
/// themselves cannot be because rotation renames them.
const LOCK: &str = "activity.lock";

/// How much of the end of a file is read at a time when walking backwards for
/// the newest records. The panel asks for a couple of hundred and a record is
/// a few hundred bytes, so the first window answers unless the lines are far
/// longer than a record — a full file at [`ROTATE_AT_BYTES`] is not parsed
/// whole to hand back its last page.
const TAIL_CHUNK: u64 = 64 * 1024;

/// Where the log is written inside `dir`.
pub fn current_path(dir: &Path) -> PathBuf {
    dir.join(CURRENT)
}

/// Where the rotated generation sits inside `dir`.
pub fn previous_path(dir: &Path) -> PathBuf {
    dir.join(PREVIOUS)
}

/// Where the writers' lock sits inside `dir`.
pub fn lock_path(dir: &Path) -> PathBuf {
    dir.join(LOCK)
}

/// Opens the lock file, creating `dir` and the file if they are not there.
///
/// Public because everything the harness writes into the data folder takes the
/// same lock: one file to contend on, whatever is being written.
pub fn open_lock(dir: &Path) -> StorageResult<File> {
    std::fs::create_dir_all(dir)?;
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path(dir))?;
    Ok(file)
}

/// Appends one record.
///
/// The line is serialised in full before the file is opened, so the handle is
/// held for one `write_all` and a concurrent appender cannot land inside it.
pub fn append(dir: &Path, record: &ActivityRecord) -> StorageResult<()> {
    let mut line = serde_json::to_vec(record)?;
    line.push(b'\n');

    let lock = open_lock(dir)?;
    lock.lock_shared()?;

    if is_full(dir, line.len() as u64) {
        // The upgrade cannot be atomic, so another writer may rotate in the gap
        // between dropping the shared lock and holding the exclusive one. The
        // second look is what stops this rotation renaming a file that is now
        // nearly empty over the generation that one just filled.
        lock.unlock()?;
        lock.lock()?;
        if is_full(dir, line.len() as u64) {
            std::fs::rename(current_path(dir), previous_path(dir))?;
        }
    }

    let written = write_line(dir, &line);
    let _ = lock.unlock();
    written
}

/// Appends one already-serialised line to the current file.
fn write_line(dir: &Path, line: &[u8]) -> StorageResult<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(current_path(dir))?;
    file.write_all(line)?;
    Ok(())
}

/// The newest `limit` records, newest first.
///
/// Reads the rotated generation only when the current file holds fewer than
/// `limit`, so the common call touches one file. A line that does not parse is
/// skipped and the rest are returned: an unreadable log is not an error the
/// user is shown, it is a shorter list.
///
/// Holds the shared lock, so a read that lands mid-rotation sees one generation
/// or the other rather than a file being renamed out from under it.
pub fn read_recent(dir: &Path, limit: usize) -> Vec<ActivityRecord> {
    if limit == 0 {
        return Vec::new();
    }
    let _guard = shared_guard(dir);
    let mut newest = tail_of(&current_path(dir), limit);
    if newest.len() < limit {
        let wanted = limit - newest.len();
        let mut older = tail_of(&previous_path(dir), wanted);
        older.append(&mut newest);
        newest = older;
    }
    newest.reverse();
    newest
}

/// Forgets both generations.
///
/// A file that is not there is already cleared, so its absence is not an error.
/// The lock file stays: it is what the writers contend on, not a generation.
pub fn clear(dir: &Path) -> StorageResult<()> {
    // Nothing to clear means nothing to lock, and a folder being emptied is not
    // a folder to create a lock file in.
    if !current_path(dir).exists() && !previous_path(dir).exists() {
        return Ok(());
    }
    let lock = open_lock(dir)?;
    lock.lock()?;
    let removed = remove_generations(dir);
    let _ = lock.unlock();
    removed
}

fn remove_generations(dir: &Path) -> StorageResult<()> {
    for path in [current_path(dir), previous_path(dir)] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// The shared lock, held for as long as the returned handle lives.
///
/// Opens the lock file without creating it, so reading the folder never writes
/// to it: before the first append there is no writer to contend with, and
/// `None` is the honest answer. `None` also covers a folder that cannot be
/// locked, which leaves a read unguarded rather than empty-handed.
pub fn shared_guard(dir: &Path) -> Option<File> {
    let file = OpenOptions::new().read(true).open(lock_path(dir)).ok()?;
    file.lock_shared().ok()?;
    Some(file)
}

/// Whether the line about to be written would take the current file past the
/// cap. Called only with the lock held.
fn is_full(dir: &Path, incoming: u64) -> bool {
    let Ok(meta) = std::fs::metadata(current_path(dir)) else {
        return false;
    };
    meta.len() + incoming > ROTATE_AT_BYTES
}

/// The last `limit` parseable records of one file, oldest first.
fn tail_of(path: &Path, limit: usize) -> Vec<ActivityRecord> {
    let Some((bytes, from_start)) = tail_bytes(path, limit) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();

    // A window that did not reach the start of the file cut its first line in
    // half, so that one is dropped rather than handed to the parser.
    let usable = if from_start || lines.is_empty() {
        &lines[..]
    } else {
        &lines[1..]
    };

    let mut newest: Vec<ActivityRecord> = usable
        .iter()
        .rev()
        .filter_map(|line| serde_json::from_str::<ActivityRecord>(line).ok())
        .take(limit)
        .collect();
    newest.reverse();
    newest
}

/// The end of `path`, wide enough to hold `limit` whole lines, and whether the
/// window reached the start of the file.
///
/// Doubles the window until it spans more than `limit` line endings, so a log
/// of unusually long records still answers in full rather than short.
fn tail_bytes(path: &Path, limit: usize) -> Option<(Vec<u8>, bool)> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut window = TAIL_CHUNK;

    loop {
        let start = len.saturating_sub(window);
        file.seek(SeekFrom::Start(start)).ok()?;

        let span = len - start;
        let mut bytes = Vec::with_capacity(span as usize);
        Read::by_ref(&mut file)
            .take(span)
            .read_to_end(&mut bytes)
            .ok()?;

        let from_start = start == 0;
        if from_start || bytes.iter().filter(|byte| **byte == b'\n').count() > limit {
            return Some((bytes, from_start));
        }
        window = window.saturating_mul(2);
    }
}
