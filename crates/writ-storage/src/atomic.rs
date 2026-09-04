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
//! Unix so the rename itself survives a crash.

use std::io::{self, Write};
use std::path::Path;

use tempfile::{Builder, NamedTempFile};
use writ_core::workspace::TEMP_FILE_PREFIX;

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

    tmp.persist(target).map_err(|e| e.error)?;

    #[cfg(unix)]
    {
        if let Ok(dir_handle) = std::fs::File::open(dir) {
            let _ = dir_handle.sync_all();
        }
    }

    Ok(())
}
