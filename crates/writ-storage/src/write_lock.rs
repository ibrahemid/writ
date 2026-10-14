//! The advisory lock Writ's processes take around the compare and the rename
//! of a guarded write (ADR-031 rule 1.4).
//!
//! The app and `writ mcp` both write notes, each through the guard, and the
//! guard's compare and its rename are two steps: a write from one process
//! landing between the other's compare and its rename was replaced with no
//! copy left of it (AM-40). Both hold this lock across those two steps, so the
//! second writer compares against what the first one wrote.
//!
//! The lock is a file in the data folder that carries no data. Nothing is
//! written to it, nothing reads it, and the app learns of a program's write the
//! way it always has, through the folder watcher (ADR-033). What it is held
//! around is a read of the note and a rename, so it is held for microseconds:
//! the replacement is written and synced before it is taken.
//!
//! A writer that cannot have it writes without it rather than not at all: a
//! lock file that will not open, a data folder that is not writable, or a
//! holder that has not let go within [`WAIT`] is logged, and the save lands
//! guarded as it did before the lock existed.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tracing::warn;

/// The lock's file name in the data folder.
pub const WRITE_LOCK_FILE: &str = "write.lock";

/// How long a writer waits for another to let go before writing without the
/// lock. A holder lets go after one read and one rename; one that has not
/// within this is stopped or hung, and a save is not held behind it.
pub const WAIT: Duration = Duration::from_secs(2);

/// How long a waiting writer sleeps between tries.
const RETRY: Duration = Duration::from_micros(200);

/// The lock every Writ process writing into one data folder's notes shares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteLock {
    path: PathBuf,
}

/// The lock while it is held; dropping it lets go.
#[derive(Debug)]
pub struct HeldWriteLock {
    file: File,
}

impl WriteLock {
    /// The lock in the data folder `writ_dir`, the one the app and every
    /// `writ` process started on that folder share.
    pub fn in_data_folder(writ_dir: &Path) -> Self {
        Self {
            path: writ_dir.join(WRITE_LOCK_FILE),
        }
    }

    /// The lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Holds the lock until the answer is dropped, or `None` when it could not
    /// be had within [`WAIT`].
    pub fn hold(&self) -> Option<HeldWriteLock> {
        self.hold_within(WAIT)
    }

    /// [`WriteLock::hold`], waiting up to `wait`.
    ///
    /// The file is opened for each hold rather than once per process: the lock
    /// belongs to an open file, so two threads of one process holding two opens
    /// exclude each other as two processes do.
    pub fn hold_within(&self, wait: Duration) -> Option<HeldWriteLock> {
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&self.path)
        {
            Ok(file) => file,
            Err(error) => {
                warn!(path = %self.path.display(), error = %error, "the write lock could not be opened; writing without it");
                return None;
            }
        };
        let deadline = Instant::now() + wait;
        loop {
            match file.try_lock() {
                Ok(()) => return Some(HeldWriteLock { file }),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(RETRY)
                }
                Err(TryLockError::WouldBlock) => {
                    warn!(path = %self.path.display(), waited_ms = wait.as_millis() as u64, "the write lock was not let go; writing without it");
                    return None;
                }
                Err(TryLockError::Error(error)) => {
                    warn!(path = %self.path.display(), error = %error, "the write lock could not be taken; writing without it");
                    return None;
                }
            }
        }
    }
}

impl Drop for HeldWriteLock {
    fn drop(&mut self) {
        // Closing the file lets go too, but Windows frees a lock left on a
        // closed handle when it gets round to it, not at the close.
        let _ = self.file.unlock();
    }
}
