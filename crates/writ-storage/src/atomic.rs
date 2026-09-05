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
//!
//! Replacing a file rather than writing through it means everything the
//! filesystem knows about the destination has to be carried across by hand:
//! its permission bits, its extended attributes (Finder tags, colour labels,
//! whatever a tool has hung on the file) and, on macOS, the date it was
//! created. Two destinations cannot be replaced safely at all — one the user
//! has under a second name, and one they have marked unwritable — and both
//! are refused before a temp file is created.

use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

use tempfile::{Builder, NamedTempFile};
use thiserror::Error;
use writ_core::workspace::TEMP_FILE_PREFIX;

/// Why [`write_atomic`] would not replace the file it was aimed at.
///
/// The two refusals are separate from an I/O failure because they are answers
/// rather than accidents: nothing about the machine went wrong, and writing
/// the same bytes again produces the same result. The caller turns them into
/// the message the editor shows ([`crate::errors::StorageError`]).
#[derive(Debug, Error)]
pub enum AtomicWriteError {
    /// The destination is one of several names for the same file.
    ///
    /// Replacing it by rename would leave the other names pointing at the old
    /// content, which is the opposite of what somebody who linked a file
    /// asked for.
    #[error("the file is reachable under {links} names")]
    HardLinked {
        /// How many names point at the file, as the filesystem counts them.
        links: u64,
    },

    /// The destination, or the folder holding it, cannot be written.
    #[error("the file or its folder cannot be written")]
    ReadOnly,

    /// Anything the filesystem reported that is not one of the two above.
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

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

/// Answers whether the file at `target` may be replaced at all.
///
/// Called before the temp file is created, so a refusal costs nothing and
/// leaves no sibling behind. A missing destination is writable by definition:
/// the save creates it, and the folder's own refusal comes from the temp file
/// failing to be created.
///
/// The check reads the entry at `target` itself rather than what it points at.
/// A symlink is replaced by the save rather than written through
/// ([`write_atomic`]), so the link, not its target, is the file at risk.
///
/// # Errors
///
/// [`AtomicWriteError::HardLinked`] when more than one name points at the
/// file, and [`AtomicWriteError::ReadOnly`] when it is marked unwritable.
pub fn refuse_unreplaceable_destination(target: &Path) -> Result<(), AtomicWriteError> {
    let Ok(metadata) = std::fs::symlink_metadata(target) else {
        return Ok(());
    };
    if !metadata.is_file() {
        return Ok(());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let links = metadata.nlink();
        if links > 1 {
            return Err(AtomicWriteError::HardLinked { links });
        }
        // The owner's write bit, not `readonly()`: `chmod 444` is what a user
        // reaches for, and a POSIX rename over a 0444 file succeeds, so the
        // destination has to be asked about rather than the failure waited
        // for.
        if metadata.permissions().mode() & 0o200 == 0 {
            return Err(AtomicWriteError::ReadOnly);
        }
    }

    // macOS `uchg`: the user-immutable flag, which Finder shows as "Locked".
    // A rename over a locked file fails with EPERM, which would otherwise
    // reach the editor as a bare I/O error.
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;

        const UF_IMMUTABLE: u32 = 0x0000_0002;
        if metadata.st_flags() & UF_IMMUTABLE != 0 {
            return Err(AtomicWriteError::ReadOnly);
        }
    }

    // Windows has no stable way to read a file's link count from safe std —
    // `MetadataExt::number_of_links` is unstable — so a hard-linked
    // destination there is replaced rather than refused. The read-only
    // attribute is checked, and it is the one users actually set.
    #[cfg(windows)]
    {
        if metadata.permissions().readonly() {
            return Err(AtomicWriteError::ReadOnly);
        }
    }

    Ok(())
}

/// The platform calls that carry a file's metadata across a replacement.
///
/// The one place in this crate that reaches past safe Rust, and the reason
/// `lib.rs` denies rather than forbids it. Extended attributes and a file's
/// creation date have no safe API in `std`, and both are lost by the rename a
/// save performs, so the alternative to these calls is losing a user's Finder
/// tags and the date every note says it was written.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod metadata {
    #![allow(unsafe_code)]

    use std::path::Path;

    /// Copies the destination's extended attributes onto the replacement.
    ///
    /// Finder tags, colour labels, "where from" records and whatever else a tool
    /// has hung on the file live here, and a rename would drop every one of them.
    /// Best effort throughout: an attribute that cannot be read or written is
    /// logged and skipped, because losing a colour label is not a reason to lose
    /// a save.
    pub(super) fn inherit_xattrs(target: &Path, replacement: &std::fs::File) {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::io::AsRawFd;

        use tracing::debug;

        // Records of where the file came from rather than what it holds. Carrying
        // the quarantine attribute forward would have Gatekeeper keep asking about
        // a file the user has been editing all along, and provenance is the same
        // shape of claim.
        const NOT_COPIED: &[&[u8]] = &[b"com.apple.quarantine", b"com.apple.provenance"];

        let Ok(path) = CString::new(target.as_os_str().as_bytes()) else {
            return;
        };

        let Some(names) = list_xattr_names(&path) else {
            return;
        };

        for name in names {
            if NOT_COPIED.contains(&name.as_bytes()) {
                continue;
            }
            let Some(value) = read_xattr(&path, &name) else {
                debug!(file = %target.display(), attribute = ?name, "an extended attribute could not be read");
                continue;
            };
            if !write_xattr(replacement.as_raw_fd(), &name, &value) {
                debug!(file = %target.display(), attribute = ?name, "an extended attribute could not be carried over");
            }
        }
    }

    /// Every extended attribute name on `path`, or `None` when the file has none
    /// and when the platform refused to say.
    ///
    /// A refusal is logged, because it costs the file every attribute it has
    /// and nothing else in the save would say so. An empty list is not: a file
    /// with no attributes is the ordinary case.
    fn list_xattr_names(path: &std::ffi::CString) -> Option<Vec<std::ffi::CString>> {
        use std::ffi::CString;

        // SAFETY: `path` is a valid NUL-terminated C string, and the size probe
        // passes a null buffer with length 0, which is how both platforms are
        // asked how much room the answer needs.
        let size = unsafe { list_xattr(path.as_ptr(), std::ptr::null_mut(), 0) };
        if size < 0 {
            tracing::debug!(
                file = %path.to_string_lossy(),
                error = %std::io::Error::last_os_error(),
                "the file's extended attributes could not be listed"
            );
            return None;
        }
        if size == 0 {
            return None;
        }

        let mut buffer = vec![0u8; size as usize];
        // SAFETY: `buffer` has exactly the length just reported.
        let written =
            unsafe { list_xattr(path.as_ptr(), buffer.as_mut_ptr().cast(), buffer.len()) };
        if written < 0 {
            tracing::debug!(
                file = %path.to_string_lossy(),
                error = %std::io::Error::last_os_error(),
                "the file's extended attributes could not be read back"
            );
            return None;
        }
        if written == 0 {
            return None;
        }

        Some(
            buffer[..written as usize]
                .split(|byte| *byte == 0)
                .filter(|name| !name.is_empty())
                .filter_map(|name| CString::new(name).ok())
                .collect(),
        )
    }

    /// One extended attribute's value.
    fn read_xattr(path: &std::ffi::CString, name: &std::ffi::CString) -> Option<Vec<u8>> {
        // SAFETY: both strings are valid and NUL-terminated; the size probe passes
        // a null buffer with length 0.
        let size = unsafe { get_xattr(path.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0) };
        if size < 0 {
            return None;
        }

        let mut value = vec![0u8; size as usize];
        // SAFETY: `value` has exactly the length just reported.
        let written = unsafe {
            get_xattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
            )
        };
        if written < 0 {
            return None;
        }
        value.truncate(written as usize);
        Some(value)
    }

    /// Hangs one extended attribute on an open file. `true` when it landed.
    fn write_xattr(fd: std::os::unix::io::RawFd, name: &std::ffi::CString, value: &[u8]) -> bool {
        // SAFETY: `fd` is the temp file this function was handed, still open;
        // `name` is NUL-terminated and `value` is read for exactly its length.
        unsafe { set_xattr(fd, name.as_ptr(), value.as_ptr().cast(), value.len()) == 0 }
    }

    // The three calls differ between the two platforms only in the trailing
    // arguments macOS adds, so each is wrapped once here and the copying code
    // above is written against one shape.

    #[cfg(target_os = "macos")]
    unsafe fn list_xattr(path: *const libc::c_char, list: *mut libc::c_char, size: usize) -> isize {
        libc::listxattr(path, list, size, 0)
    }

    #[cfg(target_os = "linux")]
    unsafe fn list_xattr(path: *const libc::c_char, list: *mut libc::c_char, size: usize) -> isize {
        libc::listxattr(path, list, size)
    }

    #[cfg(target_os = "macos")]
    unsafe fn get_xattr(
        path: *const libc::c_char,
        name: *const libc::c_char,
        value: *mut libc::c_void,
        size: usize,
    ) -> isize {
        libc::getxattr(path, name, value, size, 0, 0)
    }

    #[cfg(target_os = "linux")]
    unsafe fn get_xattr(
        path: *const libc::c_char,
        name: *const libc::c_char,
        value: *mut libc::c_void,
        size: usize,
    ) -> isize {
        libc::getxattr(path, name, value, size)
    }

    #[cfg(target_os = "macos")]
    unsafe fn set_xattr(
        fd: libc::c_int,
        name: *const libc::c_char,
        value: *const libc::c_void,
        size: usize,
    ) -> libc::c_int {
        libc::fsetxattr(fd, name, value, size, 0, 0)
    }

    #[cfg(target_os = "linux")]
    unsafe fn set_xattr(
        fd: libc::c_int,
        name: *const libc::c_char,
        value: *const libc::c_void,
        size: usize,
    ) -> libc::c_int {
        libc::fsetxattr(fd, name, value, size, 0)
    }

    /// The date the destination was created, read before it is replaced.
    ///
    /// macOS keeps a creation date per file and Finder shows it. A rename gives
    /// the note the temp file's date, so without this every save would report a
    /// note the user started last year as created moments ago.
    #[cfg(target_os = "macos")]
    pub(super) fn birthtime_of(target: &Path) -> Option<libc::timespec> {
        use std::os::macos::fs::MetadataExt;

        let metadata = std::fs::metadata(target).ok()?;
        Some(libc::timespec {
            tv_sec: metadata.st_birthtime(),
            tv_nsec: metadata.st_birthtime_nsec(),
        })
    }

    /// Puts the creation date back on the file now sitting at `target`.
    ///
    /// Best effort: a filesystem that does not carry creation dates, or a path
    /// that cannot be encoded, leaves the date the rename gave it.
    #[cfg(target_os = "macos")]
    pub(super) fn restore_birthtime(target: &Path, birthtime: libc::timespec) {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        use tracing::debug;

        // `sys/attr.h`. Not in `libc`, so the two values the call needs are named
        // here rather than spelled out at the call site.
        const ATTR_BIT_MAP_COUNT: u16 = 5;
        const ATTR_CMN_CRTIME: u32 = 0x0000_0200;

        let Ok(path) = CString::new(target.as_os_str().as_bytes()) else {
            return;
        };

        let mut attributes = libc::attrlist {
            bitmapcount: ATTR_BIT_MAP_COUNT,
            reserved: 0,
            commonattr: ATTR_CMN_CRTIME,
            volattr: 0,
            dirattr: 0,
            fileattr: 0,
            forkattr: 0,
        };
        let mut value = birthtime;

        // SAFETY: `attributes` asks for exactly one attribute and `value` is the
        // `timespec` that attribute takes, so the buffer and the size declared for
        // it are what the kernel will read.
        let result = unsafe {
            libc::setattrlist(
                path.as_ptr(),
                std::ptr::addr_of_mut!(attributes).cast(),
                std::ptr::addr_of_mut!(value).cast(),
                std::mem::size_of::<libc::timespec>(),
                0,
            )
        };
        if result != 0 {
            debug!(
                file = %target.display(),
                error = %std::io::Error::last_os_error(),
                "the file's creation date could not be put back"
            );
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
use metadata::inherit_xattrs;
#[cfg(target_os = "macos")]
use metadata::{birthtime_of, restore_birthtime};

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
/// fsyncs the parent directory afterward on Unix targets. What the
/// destination carried comes across with it: its permission bits, its
/// extended attributes on macOS and Linux, and its creation date on macOS.
/// Attributes are best effort — one that will not copy is logged and the save
/// still lands.
///
/// `target` is taken literally: a symlink at that path is replaced by a
/// regular file rather than written through. Callers that mean the linked
/// file resolve the path first — the external-file open path canonicalizes
/// before it records `source_path`, so saves land on the real file.
///
/// # Errors
///
/// [`AtomicWriteError::HardLinked`] when the destination is one of several
/// names for the same file, and [`AtomicWriteError::ReadOnly`] when the
/// destination or its folder is not writable; neither creates a temp file.
/// [`AtomicWriteError::Io`] when the parent directory cannot be resolved, the
/// temp file cannot be written, the fsync fails, or the rename fails. On any
/// failure the destination at `target` is left untouched.
///
/// On Windows the rename is retried while the destination is held open by
/// another program ([`should_retry_persist`]); a failure that outlives the
/// budget reaches the caller as [`AtomicWriteError::Io`] carrying
/// [`io::ErrorKind::ResourceBusy`].
pub fn write_atomic(target: &Path, bytes: &[u8]) -> Result<(), AtomicWriteError> {
    let dir = target.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("target path has no parent directory: {}", target.display()),
        )
    })?;

    refuse_unreplaceable_destination(target)?;

    // A folder the user cannot write is where the temp file fails, and it
    // fails before anything has been created. Reading the folder's mode
    // instead would answer for the wrong thing wherever a directory's
    // permission bits are not the whole story.
    let mut tmp = match temp_sibling(dir) {
        Ok(tmp) => tmp,
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            return Err(AtomicWriteError::ReadOnly)
        }
        Err(e) => return Err(AtomicWriteError::Io(e)),
    };

    tmp.as_file_mut().write_all(bytes)?;
    tmp.as_file_mut().sync_all()?;

    #[cfg(unix)]
    inherit_mode(target, tmp.as_file());

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    inherit_xattrs(target, tmp.as_file());

    #[cfg(target_os = "macos")]
    let birthtime = birthtime_of(target);

    persist_replacement(tmp, target)?;

    #[cfg(target_os = "macos")]
    if let Some(birthtime) = birthtime {
        restore_birthtime(target, birthtime);
    }

    #[cfg(unix)]
    {
        if let Ok(dir_handle) = std::fs::File::open(dir) {
            let _ = dir_handle.sync_all();
        }
    }

    Ok(())
}
