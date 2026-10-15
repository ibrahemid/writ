//! The data-folder lock, and the requests a second launch leaves for the
//! process holding it.
//!
//! `<data dir>/instance.lock` carries no data: holding an exclusive advisory
//! lock on it is what makes a process the one Writ for that folder. The kernel
//! drops the lock when the process ends, however it ends, so a crash never
//! leaves the folder claimed. `File::try_lock` has been in std since 1.89,
//! this workspace's minimum, so the lock costs no dependency.
//!
//! A second launch writes an [`OpenRequest`] into `<data dir>/open-requests/`
//! and exits. Files rather than a socket: the folder is one every platform can
//! write, a Unix socket path is capped at 104 bytes on macOS and std has none
//! on Windows, and a request written while the running instance is still
//! starting waits on disk until it looks.
//!
//! `writ --wait` leaves an empty marker in `<data dir>/waits/` named by its
//! token and names the token in its request. The running instance removes the
//! marker when the last tab the request opened closes, and the waiting command
//! returns when the marker is gone. Nothing listens for the marker at rest: the
//! command looks only while it waits (ADR-017).

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;
use writ_core::instance::{is_wait_token, OpenRequest};

use crate::atomic::write_atomic;
use crate::errors::StorageResult;

/// The lock file inside the data folder.
const LOCK_FILE: &str = "instance.lock";

/// The folder requests from a second launch land in.
const REQUESTS_DIR: &str = "open-requests";

/// The folder a waiting `writ` leaves its marker in.
const WAITS_DIR: &str = "waits";

/// Why the data folder could not be claimed.
#[derive(Error, Debug)]
pub enum InstanceLockError {
    /// Another process holds the folder.
    #[error("another Writ is running on this data folder")]
    Held,
    /// The lock file could not be opened or the filesystem does not lock.
    #[error("the data folder lock is unavailable: {0}")]
    Unavailable(#[source] io::Error),
}

/// The claim on a data folder, released when this is dropped or the process
/// ends.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

/// Where the lock sits inside `writ_dir`.
pub fn lock_path(writ_dir: &Path) -> PathBuf {
    writ_dir.join(LOCK_FILE)
}

/// Where requests from a second launch land inside `writ_dir`.
pub fn requests_dir(writ_dir: &Path) -> PathBuf {
    writ_dir.join(REQUESTS_DIR)
}

/// Claims `writ_dir` for this process, creating the folder and the lock file
/// when they are not there.
///
/// # Errors
///
/// [`InstanceLockError::Held`] when another process holds the lock, and
/// [`InstanceLockError::Unavailable`] when the file cannot be opened or the
/// filesystem refuses advisory locks (some network mounts).
pub fn acquire(writ_dir: &Path) -> Result<InstanceLock, InstanceLockError> {
    std::fs::create_dir_all(writ_dir).map_err(InstanceLockError::Unavailable)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path(writ_dir))
        .map_err(InstanceLockError::Unavailable)?;
    match file.try_lock() {
        Ok(()) => Ok(InstanceLock { _file: file }),
        Err(TryLockError::WouldBlock) => Err(InstanceLockError::Held),
        Err(TryLockError::Error(error)) => Err(InstanceLockError::Unavailable(error)),
    }
}

/// Where a waiting `writ` leaves its markers inside `writ_dir`.
pub fn waits_dir(writ_dir: &Path) -> PathBuf {
    writ_dir.join(WAITS_DIR)
}

/// A token no other command waiting on this folder holds: this process's id,
/// the clock, and a counter.
pub fn new_wait_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{:x}-{nanos:x}-{sequence:x}", std::process::id())
}

/// Leaves the marker a `writ --wait` holding `token` watches, returning its
/// path.
///
/// # Errors
///
/// [`crate::errors::StorageError`] when the token could name another file, or
/// the folder or the file cannot be created.
pub fn create_wait_marker(writ_dir: &Path, token: &str) -> StorageResult<PathBuf> {
    if !is_wait_token(token) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a wait token").into());
    }
    let dir = waits_dir(writ_dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(token);
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    Ok(path)
}

/// Answers the `writ --wait` holding `token` by removing its marker.
///
/// A token that could name another file is refused, and a marker already gone
/// is answered: the command stopped waiting, or another look removed it.
pub fn release_wait(writ_dir: &Path, token: &str) {
    if !is_wait_token(token) {
        return;
    }
    match std::fs::remove_file(waits_dir(writ_dir).join(token)) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(%error, "a waiting command could not be answered"),
    }
}

/// Answers every `writ --wait` left from an earlier run.
///
/// Called once the folder is claimed: the instance those commands waited on
/// has ended, so the tabs they waited for are closed. A wait named by a
/// request still in `open-requests/` is kept: that request is what started
/// this instance, and its tab has not opened yet.
pub fn clear_waits(writ_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(waits_dir(writ_dir)) else {
        return;
    };
    let pending = pending_wait_tokens(writ_dir);
    for entry in entries.flatten() {
        if let Some(token) = entry.file_name().to_str() {
            if !pending.contains(token) {
                release_wait(writ_dir, token);
            }
        }
    }
}

/// The wait tokens the requests in `writ_dir` name, read without taking them.
fn pending_wait_tokens(writ_dir: &Path) -> std::collections::HashSet<String> {
    let Ok(entries) = std::fs::read_dir(requests_dir(writ_dir)) else {
        return std::collections::HashSet::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_request_file(path))
        .filter_map(|path| std::fs::read(path).ok())
        .filter_map(|bytes| serde_json::from_slice::<OpenRequest>(&bytes).ok())
        .filter_map(|request| request.wait_token().map(str::to_string))
        .collect()
}

/// Separates two requests one process writes in the same millisecond.
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Leaves `request` for the process holding `writ_dir`, returning the file it
/// was written to.
///
/// Written whole through [`write_atomic`]: the temp file it renames from is a
/// dot name, which [`take_requests`] never reads, so a reader never sees half
/// a request.
///
/// # Errors
///
/// [`crate::errors::StorageError`] when the folder cannot be created or the
/// file cannot be written.
pub fn send_request(writ_dir: &Path, request: &OpenRequest) -> StorageResult<PathBuf> {
    let dir = requests_dir(writ_dir);
    std::fs::create_dir_all(&dir)?;
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = format!(
        "{:020}-{}-{sequence}.json",
        request.sent_at_ms.max(0),
        std::process::id()
    );
    let path = dir.join(name);
    write_atomic(&path, &serde_json::to_vec(request)?)?;
    Ok(path)
}

/// Takes every request waiting in `writ_dir`, oldest first, removing each
/// file as it is read.
///
/// A file that does not parse is removed and skipped: nothing that wrote it
/// is waiting on an answer, and leaving it would make every later look read
/// it again. A file that cannot be removed is still returned, once: the next
/// look finds it again, and opening a file twice costs a tab already open
/// coming forward.
pub fn take_requests(writ_dir: &Path) -> Vec<OpenRequest> {
    let Ok(entries) = std::fs::read_dir(requests_dir(writ_dir)) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_request_file(path))
        .collect();
    files.sort();
    let mut requests = Vec::with_capacity(files.len());
    for path in files {
        let parsed = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<OpenRequest>(&bytes).ok());
        if let Err(error) = std::fs::remove_file(&path) {
            if error.kind() == io::ErrorKind::NotFound {
                // Another look took it first.
                continue;
            }
            tracing::warn!(%error, "an open request could not be removed after reading");
        }
        match parsed {
            Some(request) => requests.push(request),
            None => tracing::warn!("an open request did not parse and was dropped"),
        }
    }
    requests
}

/// Whether `path` names a finished request rather than a temp file on its way
/// to becoming one.
fn is_request_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    !name.starts_with('.') && name.ends_with(".json")
}
