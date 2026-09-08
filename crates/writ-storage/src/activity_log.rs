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
//! generation is kept. A line that does not parse is skipped, never fatal —
//! the one thing a torn write from an older build could leave behind must not
//! take the panel down with it.

use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use writ_core::activity::ActivityRecord;

use crate::errors::StorageResult;

/// Size at which the current file is rotated, in bytes (ADR-031 rule 5.4).
pub const ROTATE_AT_BYTES: u64 = 5 * 1024 * 1024;

/// The file being appended to.
const CURRENT: &str = "activity.jsonl";

/// The one generation kept behind it.
const PREVIOUS: &str = "activity.1.jsonl";

/// Where the log is written inside `dir`.
pub fn current_path(dir: &Path) -> PathBuf {
    dir.join(CURRENT)
}

/// Where the rotated generation sits inside `dir`.
pub fn previous_path(dir: &Path) -> PathBuf {
    dir.join(PREVIOUS)
}

/// Appends one record.
///
/// The line is serialised in full before the file is opened, so the handle is
/// held for one `write_all` and a concurrent appender cannot land inside it.
pub fn append(dir: &Path, record: &ActivityRecord) -> StorageResult<()> {
    let mut line = serde_json::to_vec(record)?;
    line.push(b'\n');

    std::fs::create_dir_all(dir)?;
    rotate_if_full(dir, line.len() as u64)?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(current_path(dir))?;
    file.write_all(&line)?;
    Ok(())
}

/// The newest `limit` records, newest first.
///
/// Reads the rotated generation only when the current file holds fewer than
/// `limit`, so the common call touches one file. A line that does not parse is
/// skipped and the rest are returned: an unreadable log is not an error the
/// user is shown, it is a shorter list.
pub fn read_recent(dir: &Path, limit: usize) -> Vec<ActivityRecord> {
    if limit == 0 {
        return Vec::new();
    }
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
pub fn clear(dir: &Path) -> StorageResult<()> {
    for path in [current_path(dir), previous_path(dir)] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Rotates when the line about to be written would take the file past the cap.
///
/// Two processes can decide to rotate at the same moment. The rename is atomic
/// and the loser overwrites the same generation with a file of the same age, so
/// the worst case is one generation shorter than it could have been, never a
/// lost current file.
fn rotate_if_full(dir: &Path, incoming: u64) -> StorageResult<()> {
    let current = current_path(dir);
    let Ok(meta) = std::fs::metadata(&current) else {
        return Ok(());
    };
    if meta.len() + incoming <= ROTATE_AT_BYTES {
        return Ok(());
    }
    std::fs::rename(&current, previous_path(dir))?;
    Ok(())
}

/// The last `limit` parseable records of one file, oldest first.
fn tail_of(path: &Path, limit: usize) -> Vec<ActivityRecord> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut kept: VecDeque<ActivityRecord> = VecDeque::with_capacity(limit.min(1024));
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { continue };
        let Ok(record) = serde_json::from_str::<ActivityRecord>(&line) else {
            continue;
        };
        if kept.len() == limit {
            kept.pop_front();
        }
        kept.push_back(record);
    }
    kept.into()
}
