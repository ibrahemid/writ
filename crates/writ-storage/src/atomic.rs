//! Crash-safe file writes via temp-file + fsync + rename.
//!
//! Editors must never present a truncated or partially-written file
//! after a crash, power loss, or process kill. `std::fs::write`
//! truncates the destination before writing, so a crash between
//! truncate and `write_all` leaves the user with corrupted content and
//! no recourse.
//!
//! [`write_atomic`] writes the new bytes to a sibling temp file in the
//! same directory, fsyncs the file's contents, then renames it into
//! place. On POSIX, `rename(2)` over an existing destination is atomic.
//! On Windows, [`tempfile::NamedTempFile::persist`] uses `ReplaceFile`
//! to provide the same guarantee. The parent directory is fsynced on
//! Unix so the rename itself survives a crash. Windows refuses that rename
//! while another program holds the destination open, so it is retried for a
//! fraction of a second before the save is reported as failed.

use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

use tempfile::{Builder, NamedTempFile};
use writ_core::workspace::TEMP_FILE_PREFIX;

/// `ERROR_ACCESS_DENIED`, one of the codes a Windows rename over a file
/// another handle holds open comes back as.
const ERROR_ACCESS_DENIED: i32 = 5;

/// `ERROR_SHARING_VIOLATION`, the other code that refusal can carry.
const ERROR_SHARING_VIOLATION: i32 = 32;

/// How many times a save asks Windows to move the replacement into place
/// before it gives up.
///
/// A rename over a file that a watcher, a sync client, or a virus scanner has
/// open without `FILE_SHARE_DELETE` fails outright. Those handles are held for
/// a few milliseconds at a time, so a save that would be lost lands on a later
/// attempt.
pub const PERSIST_ATTEMPTS: u32 = 10;

/// Longest a single wait between two attempts grows to.
const PERSIST_BACKOFF_CAP: Duration = Duration::from_millis(50);

/// Whether a rename that failed with `raw_os_error` on `attempt` is worth
/// another try.
///
/// Only the two Windows codes for a file somebody else has open retry, and
/// only while attempts remain: a refusal that is really a refusal has to reach
/// the caller rather than be waited out.
pub fn should_retry_persist(raw_os_error: Option<i32>, attempt: u32) -> bool {
    if attempt + 1 >= PERSIST_ATTEMPTS {
        return false;
    }
    is_file_in_use(raw_os_error)
}

/// How long to wait before the attempt after `attempt`.
///
/// One millisecond, doubling, capped at [`PERSIST_BACKOFF_CAP`]. The whole
/// schedule adds up to well under half a second, so a save that cannot land
/// still fails while the person is looking at it.
pub fn persist_retry_delay(attempt: u32) -> Duration {
    let millis = 1u64.checked_shl(attempt).unwrap_or(u64::MAX);
    Duration::from_millis(millis).min(PERSIST_BACKOFF_CAP)
}

/// Re-reads a rename failure that outlived the retry budget.
///
/// The codes read here are Windows raw codes. `ERROR_ACCESS_DENIED` reaches
/// `std::io` as [`io::ErrorKind::PermissionDenied`], which the editor renders
/// as a file the person is not allowed to change. Once the whole budget has
/// gone by, the likelier reading is that another program is holding the file,
/// so the failure comes back as [`io::ErrorKind::ResourceBusy`] carrying the
/// operating system's own wording as its source.
pub fn classify_persist_failure(error: io::Error) -> io::Error {
    if is_file_in_use(error.raw_os_error()) {
        return io::Error::new(io::ErrorKind::ResourceBusy, error);
    }
    error
}

fn is_file_in_use(raw_os_error: Option<i32>) -> bool {
    matches!(
        raw_os_error,
        Some(ERROR_ACCESS_DENIED) | Some(ERROR_SHARING_VIOLATION)
    )
}

/// Moves the written replacement onto `target`.
#[cfg(not(windows))]
fn persist_replacement(tmp: NamedTempFile, target: &Path) -> io::Result<()> {
    tmp.persist(target).map(|_| ()).map_err(|e| e.error)
}

/// Moves the written replacement onto `target`, retrying while Windows says
/// the destination is in use.
///
/// `persist` hands the temp file back with every failure, so the written bytes
/// stay on disk across the waits and are dropped, which deletes them, only
/// once the last attempt is gone.
#[cfg(windows)]
fn persist_replacement(mut tmp: NamedTempFile, target: &Path) -> io::Result<()> {
    let mut attempt = 0;
    loop {
        let error = match tmp.persist(target) {
            Ok(_) => return Ok(()),
            Err(refused) => {
                tmp = refused.file;
                refused.error
            }
        };
        if !should_retry_persist(error.raw_os_error(), attempt) {
            return Err(classify_persist_failure(error));
        }
        std::thread::sleep(persist_retry_delay(attempt));
        attempt += 1;
    }
}

/// Copies the destination's permission bits onto the replacement file.
///
/// `tempfile` creates the replacement 0600 and `persist` renames it over the
/// destination, so a save that did not do this would narrow a 0644 config or a
/// 0755 script to owner-only read/write the first time it was edited. Absent
/// or unreadable metadata means there is nothing to inherit (a file being
/// created) and leaves the temp file's own mode in place.
///
/// Windows is not covered: the replacement inherits the directory's ACL, and
/// carrying the read-only attribute across would make `persist` fail.
#[cfg(unix)]
fn inherit_mode(target: &Path, replacement: &std::fs::File) {
    use std::os::unix::fs::PermissionsExt;

    let Ok(metadata) = std::fs::metadata(target) else {
        return;
    };
    let mode = metadata.permissions().mode() & 0o7777;
    let _ = replacement.set_permissions(std::fs::Permissions::from_mode(mode));
}

/// Creates the temp file a save is written to before it is renamed into place.
///
/// The one place Writ names a temp file, and the reason the name is not left
/// to `tempfile`'s default: the sibling appears and disappears in the user's
/// notes folder on every save, where a sync client uploads it and the watcher
/// sees it. Both can skip a name they can match, so it starts with
/// [`TEMP_FILE_PREFIX`], which [`writ_core::workspace::is_ignored_name`]
/// answers for.
pub fn temp_sibling(dir: &Path) -> io::Result<NamedTempFile> {
    Builder::new().prefix(TEMP_FILE_PREFIX).tempfile_in(dir)
}

/// Writes `bytes` to `target` such that observers see either the old
/// content or the new content, never a partial write.
///
/// The function fsyncs the temp file before rename and best-effort
/// fsyncs the parent directory afterward on Unix targets. An existing
/// destination's permission bits carry over to the replacement.
///
/// `target` is taken literally: a symlink at that path is replaced by a
/// regular file rather than written through. Callers that mean the linked
/// file resolve the path first — the external-file open path canonicalizes
/// before it records `source_path`, so saves land on the real file.
///
/// # Errors
///
/// Returns the underlying [`io::Error`] when the parent directory
/// cannot be resolved, the temp file cannot be created or written,
/// the fsync fails, or the atomic rename into place fails. On failure
/// the destination at `target` is left untouched.
///
/// On Windows the rename is retried while the destination is held open by
/// another program ([`should_retry_persist`]); a failure that outlives the
/// budget carries [`io::ErrorKind::ResourceBusy`].
pub fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = target.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("target path has no parent directory: {}", target.display()),
        )
    })?;

    let mut tmp = temp_sibling(dir)?;
    tmp.as_file_mut().write_all(bytes)?;
    tmp.as_file_mut().sync_all()?;

    #[cfg(unix)]
    inherit_mode(target, tmp.as_file());

    persist_replacement(tmp, target)?;

    #[cfg(unix)]
    {
        if let Ok(dir_handle) = std::fs::File::open(dir) {
            let _ = dir_handle.sync_all();
        }
    }

    Ok(())
}
