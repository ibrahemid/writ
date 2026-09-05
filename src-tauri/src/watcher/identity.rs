//! Reading a file's identity from the platform.
//!
//! The mechanism half of [`writ_core::notes::identity`]: the policy decides
//! what a vanished file means, and this answers the one question it needs —
//! what the filesystem calls the file at a path, independently of the path.
//!
//! Unix answers with `dev` and `ino`, which `std` exposes on every metadata
//! read. Windows answers with `FILE_ID_INFO`, which needs an open handle and
//! is not available on FAT, exFAT, or some SMB servers. A volume that will not
//! answer gets [`FileIdentity::Fallback`], a description that cannot recognise
//! the file anywhere else, which is exactly what makes the verdict degrade
//! instead of guessing (spec W4).

use std::path::Path;

use writ_core::notes::guard::is_not_downloaded;
use writ_core::notes::identity::{FileIdentity, IdentityProbe};
use writ_storage::buffer_store::dataless_flags;

/// The platform's own answer, which is what production uses.
pub struct PlatformIdentity;

impl IdentityProbe for PlatformIdentity {
    fn identity_of(&self, path: &Path) -> Option<FileIdentity> {
        read_identity(path)
    }
}

/// The identity of the file at `path`, or `None` when there is nothing there
/// to read.
///
/// Falls back to a description of the file when the platform has no stable id
/// for it. The fallback is not reached on Unix, where every file has an inode;
/// it is reached on a Windows volume that cannot answer, which is a synced
/// notes folder on a memory card or a share.
pub fn read_identity(path: &Path) -> Option<FileIdentity> {
    if let Some(identity) = platform_identity(path) {
        return Some(identity);
    }
    fallback_identity(path)
}

/// Describes the file for a platform with no stable id to give.
///
/// A file whose bytes are not on this machine is left without an identity
/// rather than described: the description carries a hash, and hashing it would
/// make the sync provider fetch the whole file (ADR-028 §5). A note in that
/// state has nothing to compare against, which reads the same as an
/// unreadable id and degrades the same way.
fn fallback_identity(path: &Path) -> Option<FileIdentity> {
    if is_not_downloaded(dataless_flags(path)) {
        return None;
    }
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(FileIdentity::Fallback {
        path: path.to_string_lossy().into_owned(),
        size: metadata.len(),
        mtime_ms: metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|since| since.as_millis() as i64),
        hash: writ_core::hash::sha256_bytes(&bytes),
    })
}

/// When the file was created, in nanoseconds since the Unix epoch, or `None`
/// where the filesystem does not say.
///
/// `statx` reports `btime` on ext4, xfs and btrfs from Linux 4.11, and every
/// APFS and NTFS file has one. Nanoseconds rather than milliseconds because
/// the value's whole job is to separate two files, and rounding it discards
/// the separation. A birth time before the Unix epoch reads as unknown, which
/// costs nothing: no note is older than the epoch, and a volume answering that
/// is answering nonsense.
pub fn birth_nanos(metadata: &std::fs::Metadata) -> Option<u128> {
    metadata
        .created()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|since| since.as_nanos())
}

/// Unix: the device and inode every file has, and the birth time that says
/// which file is holding the inode.
#[cfg(unix)]
fn platform_identity(path: &Path) -> Option<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    Some(FileIdentity::Inode {
        dev: metadata.dev(),
        ino: metadata.ino(),
        birth_ns: birth_nanos(&metadata),
    })
}

/// Windows: `FILE_ID_INFO`, which needs a handle on the file.
///
/// `std`'s `file_index` is the older 64-bit id and is still unstable, so the
/// call is made directly. `File::open` is not what opens it: that asks for read
/// access and shares the file for reading and writing but not deleting, so a
/// probe running while Explorer renames or deletes the file would fail the very
/// operation this exists to classify. The handle here asks only to read
/// attributes, shares the file for everything including delete, and carries
/// `FILE_FLAG_BACKUP_SEMANTICS` so the call answers for a directory rather than
/// failing on it.
///
/// A volume with no file id — FAT, exFAT, some SMB servers — fails the call,
/// and the caller falls back to describing the file.
#[cfg(windows)]
fn platform_identity(path: &Path) -> Option<FileIdentity> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::FILE_ID_INFO;
    use windows::Win32::Storage::FileSystem::{FileIdInfo, GetFileInformationByHandleEx};

    /// `FILE_READ_ATTRIBUTES`: metadata, and nothing that reads the bytes.
    const READ_ATTRIBUTES: u32 = 0x0000_0080;
    /// `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`.
    const SHARE_EVERYTHING: u32 = 0x0000_0007;
    /// `FILE_FLAG_BACKUP_SEMANTICS`, which is what lets a directory open.
    const BACKUP_SEMANTICS: u32 = 0x0200_0000;

    let file = std::fs::OpenOptions::new()
        .access_mode(READ_ATTRIBUTES)
        .share_mode(SHARE_EVERYTHING)
        .custom_flags(BACKUP_SEMANTICS)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let handle = HANDLE(file.as_raw_handle());
    let mut info = FILE_ID_INFO::default();
    // SAFETY: `handle` is open for the length of the call, and the buffer is a
    // `FILE_ID_INFO` of exactly the size passed.
    unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            std::ptr::addr_of_mut!(info).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
        .ok()?;
    }
    Some(FileIdentity::Windows {
        volume: info.VolumeSerialNumber,
        index: u128::from_le_bytes(info.FileId.Identifier),
    })
}

/// A platform with neither, which is where the fallback is the only answer.
#[cfg(not(any(unix, windows)))]
fn platform_identity(_path: &Path) -> Option<FileIdentity> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use writ_core::notes::identity::{classify_delete, DeleteVerdict};

    /// A probe with no answers, which is what a volume carrying no file id
    /// looks like from here.
    struct NoIdentity;

    impl IdentityProbe for NoIdentity {
        fn identity_of(&self, _path: &Path) -> Option<FileIdentity> {
            None
        }
    }

    #[test]
    fn a_file_that_is_not_there_has_no_identity() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(read_identity(&dir.path().join("never-written.md")).is_none());
    }

    #[test]
    fn a_folder_is_not_a_file_and_has_no_identity() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(read_identity(dir.path()).is_none());
    }

    #[test]
    fn the_same_file_reads_the_same_identity_twice() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("note.md");
        std::fs::write(&path, "body").expect("write");
        assert_eq!(read_identity(&path), read_identity(&path));
    }

    #[test]
    fn two_files_in_one_folder_read_different_identities() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = dir.path().join("first.md");
        let second = dir.path().join("second.md");
        std::fs::write(&first, "body").expect("write");
        std::fs::write(&second, "body").expect("write");
        assert_ne!(read_identity(&first), read_identity(&second));
    }

    #[test]
    fn a_renamed_file_keeps_its_identity() {
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("before.md");
        let to = dir.path().join("after.md");
        std::fs::write(&from, "body").expect("write");
        let before = read_identity(&from).expect("identity");
        std::fs::rename(&from, &to).expect("rename");
        let after = read_identity(&to).expect("identity");
        assert_eq!(before, after);
        assert_eq!(
            classify_delete(&before, &[(to.clone(), after)]),
            DeleteVerdict::Moved(to)
        );
    }

    #[test]
    fn a_file_replaced_in_place_reads_a_new_identity() {
        // What a sync client, an editor and git all leave behind: a sibling
        // written and renamed over the target, so the path and the name are
        // the same and the file is a different one. The replacement is created
        // while the original is still there, which is why no filesystem can
        // hand it the original's inode number.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("note.md");
        let incoming = dir.path().join("note.md.tmp");
        std::fs::write(&path, "first").expect("write");
        std::fs::write(&incoming, "second").expect("write");
        let before = read_identity(&path).expect("identity");
        std::fs::rename(&incoming, &path).expect("replace");
        let after = read_identity(&path).expect("identity");
        assert_ne!(before, after);
    }

    #[test]
    fn an_inode_number_handed_back_out_is_not_the_file_that_had_it() {
        // ext4 gives a freed inode number to the next file created, so the id
        // read from an unrelated new file can equal the one on record. The
        // identities are built here rather than read from a disk: whether a
        // filesystem reuses the number is the host's business, and the rule
        // has to hold on every one of them.
        let before = FileIdentity::Inode {
            dev: 2049,
            ino: 2_103_239,
            birth_ns: Some(1_700_000_000_000_000_000),
        };
        let recreated = FileIdentity::Inode {
            dev: 2049,
            ino: 2_103_239,
            birth_ns: Some(1_700_000_000_004_000_000),
        };
        assert_eq!(
            classify_delete(
                &before,
                &[(PathBuf::from("/notes/unrelated.md"), recreated)]
            ),
            DeleteVerdict::Removed
        );
    }

    #[test]
    fn an_inode_with_no_birth_time_to_read_still_finds_the_file_it_names() {
        // A volume that reports no birth time answers `None` for every file on
        // it, and the inode alone is then the whole of the answer, exactly as
        // it was before the birth time was read at all.
        let before = FileIdentity::Inode {
            dev: 2049,
            ino: 2_103_239,
            birth_ns: None,
        };
        let candidate = FileIdentity::Inode {
            dev: 2049,
            ino: 2_103_239,
            birth_ns: None,
        };
        assert_eq!(
            classify_delete(&before, &[(PathBuf::from("/notes/renamed.md"), candidate)]),
            DeleteVerdict::Moved(PathBuf::from("/notes/renamed.md"))
        );
    }

    #[test]
    #[cfg(unix)]
    fn unix_answers_with_the_inode() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("note.md");
        std::fs::write(&path, "body").expect("write");
        assert!(matches!(
            read_identity(&path),
            Some(FileIdentity::Inode { .. })
        ));
    }

    #[test]
    fn a_platform_with_no_file_id_selects_the_fallback() {
        // The selection is what is asserted, never that the fallback can find
        // a moved file: it cannot, and the verdict degrading is the point.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("note.md");
        std::fs::write(&path, "body").expect("write");
        let described = fallback_identity(&path).expect("a description");
        assert!(matches!(described, FileIdentity::Fallback { .. }));
        assert!(!described.is_durable());
        assert!(NoIdentity.identity_of(&path).is_none());
        assert_eq!(
            classify_delete(&described, &[(path, described.clone())]),
            DeleteVerdict::ExternalModification
        );
    }
}
