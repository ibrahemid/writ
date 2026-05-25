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

use tempfile::NamedTempFile;

/// Writes `bytes` to `target` such that observers see either the old
/// content or the new content, never a partial write.
///
/// The function fsyncs the temp file before rename and best-effort
/// fsyncs the parent directory afterward on Unix targets.
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
            format!(
                "target path has no parent directory: {}",
                target.display()
            ),
        )
    })?;

    let mut tmp = NamedTempFile::new_in(dir)?;
    tmp.as_file_mut().write_all(bytes)?;
    tmp.as_file_mut().sync_all()?;
    tmp.persist(target).map_err(|e| e.error)?;

    #[cfg(unix)]
    {
        if let Ok(dir_handle) = std::fs::File::open(dir) {
            let _ = dir_handle.sync_all();
        }
    }

    Ok(())
}
