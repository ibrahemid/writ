//! Telling a file that moved from one that was deleted.
//!
//! A path is not a file. Moving a note in Finder and deleting it look the same
//! from a watcher: the path stops holding anything. Writ has to tell them
//! apart, because the answers are opposite — a move keeps the tab editing the
//! same note at its new path, while a delete must stop the next save
//! recreating a file the user threw away (spec W4).
//!
//! What separates them is the file's identity, which the filesystem keeps
//! independently of the name: `dev` and `ino` on Unix, `FILE_ID_INFO` on
//! Windows. A file that moved has the same identity at another path; a file
//! that was deleted has that identity nowhere.
//!
//! Reading the identity is a syscall and lives in the Tauri crate. This module
//! is the decision, so it runs the same on every platform and is tested on
//! every platform. [`IdentityProbe`] is the seam between the two.

use std::path::{Path, PathBuf};

use crate::hash::Sha256Digest;

/// What the filesystem calls a file, independently of where it sits.
///
/// The first two variants survive a rename, which is the whole point. The
/// third does not, and exists because there are real volumes that cannot
/// answer the question at all: FAT and exFAT have no stable file id, and some
/// SMB servers report one that changes. A synced notes folder on Windows is
/// exactly where that happens, so the case is a fallback rather than an error
/// ([`classify_delete`] degrades on it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileIdentity {
    /// Unix: the device the file is on and its inode number. Both are needed —
    /// an inode number is unique per volume, so two files on two volumes
    /// collide on `ino` alone.
    Inode {
        /// Device the file is on.
        dev: u64,
        /// Inode number within that device.
        ino: u64,
    },
    /// Windows: the volume serial number and the 128-bit file id
    /// `FILE_ID_INFO` reports.
    Windows {
        /// Volume serial number.
        volume: u64,
        /// File id within that volume.
        index: u128,
    },
    /// No stable id was available, so the file is described by what can be
    /// observed instead.
    ///
    /// This cannot recognise the same file at another path — the path is part
    /// of the description — which is why a note carrying one gets the degraded
    /// verdict rather than a guess.
    Fallback {
        /// The file's path, as the caller spells it.
        path: String,
        /// Length in bytes.
        size: u64,
        /// Modification time in milliseconds since the Unix epoch, when the
        /// filesystem reports one.
        mtime_ms: Option<i64>,
        /// SHA-256 of the file's bytes.
        hash: Sha256Digest,
    },
}

impl FileIdentity {
    /// Whether this identity can recognise the same file at another path.
    ///
    /// `false` for [`FileIdentity::Fallback`], which is the signal to degrade
    /// rather than guess.
    pub fn is_durable(&self) -> bool {
        !matches!(self, Self::Fallback { .. })
    }
}

/// What became of a file that is no longer where Writ left it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteVerdict {
    /// The same file is at this path now. The tab follows it.
    Moved(PathBuf),
    /// The file is gone. The tab keeps its text and stops writing to the path.
    Removed,
    /// Which of the two happened cannot be established, so neither is claimed:
    /// the change is reported as an ordinary external modification and the
    /// write guard (spec W2) governs the next save, exactly as it did before
    /// identity was tracked.
    ExternalModification,
}

/// Reads a file's identity from the filesystem.
///
/// The seam between this module and the syscalls. A test supplies identities
/// directly and never touches a disk, which is how the verdict is covered on
/// a machine that has no FAT volume and no Windows.
pub trait IdentityProbe: Send + Sync {
    /// The identity of the file at `path`, or `None` when there is nothing to
    /// read there.
    fn identity_of(&self, path: &Path) -> Option<FileIdentity>;
}

/// Decides what became of a file whose path stopped holding it.
///
/// `before` is the identity Writ recorded for the file. `candidates` are the
/// paths that could be holding it now, each with the identity read from it —
/// the files created or renamed in the same watcher batch, and the files in
/// the folder it left. Order is the caller's preference: with hard links the
/// same identity is genuinely at more than one path, and the first is taken.
///
/// Three rules and no more. A `Fallback` identity cannot recognise a file
/// elsewhere, so it degrades. A candidate carrying the same identity is the
/// file, wherever it is. Anything else is a delete — including a folder full
/// of other notes, which is the ordinary case and must not be read as
/// evidence of anything.
///
/// A sync client replacing a file (delete, then create at the same path with a
/// new id) never reaches here: the path holds a file again, so the watcher
/// classifies it as a modification before asking this question at all.
pub fn classify_delete(
    before: &FileIdentity,
    candidates: &[(PathBuf, FileIdentity)],
) -> DeleteVerdict {
    if !before.is_durable() {
        return DeleteVerdict::ExternalModification;
    }
    for (path, candidate) in candidates {
        if candidate == before {
            return DeleteVerdict::Moved(path.clone());
        }
    }
    DeleteVerdict::Removed
}

/// Whether Writ can still write to the file behind a note.
///
/// Held per open tab for the length of a session rather than in the database:
/// it describes the filesystem as it is now, and the answer at launch is read
/// from the file rather than remembered from last time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceState {
    /// The file is where the note says it is.
    Present,
    /// The file was deleted and the note still holds its text. The next save
    /// must refuse rather than recreate the file the user threw away.
    RemovedOnDisk,
}

/// What a tab knows about its file after looking at the path it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sighting {
    /// What the filesystem calls the file, as far as it will say.
    pub identity: Option<FileIdentity>,
    /// Whether Writ can still write to it.
    pub state: SourceState,
}

/// Decides what a tab records after seeing its file where the tab expects it.
///
/// Every other editor saves by writing a sibling temporary file and renaming
/// it over the target — vim, VS Code, git, rsync and every sync client — so
/// the file behind a tab is a different file after somebody else writes to it,
/// while the path is unchanged. An identity recorded once at open is stale
/// from the first such write, and a stale identity turns the next rename into
/// a deletion: nothing carries the id any more, so [`classify_delete`] says
/// [`DeleteVerdict::Removed`] and the tab stops saving to a file that is
/// sitting at its new path. So the id is re-read whenever the tab learns the
/// file changed underneath it, not only on open and after Writ's own writes.
///
/// `seen` is what the filesystem answers for the path now, and `None` covers
/// two different refusals: there is nothing there, and there is something
/// there that will not be described — a file whose bytes are not on this
/// machine is left without an id rather than hashed, because hashing it means
/// making the sync provider fetch it (ADR-028 §5). `present` is what separates
/// them, so a refusal to answer keeps the recorded id rather than blanking it.
/// Blanking it would put an evicted note in exactly the state this exists to
/// prevent.
pub fn observe_file(
    recorded: Option<FileIdentity>,
    seen: Option<FileIdentity>,
    present: bool,
) -> Sighting {
    if !present {
        return Sighting {
            identity: None,
            state: SourceState::RemovedOnDisk,
        };
    }
    Sighting {
        identity: seen.or(recorded),
        state: SourceState::Present,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inode(dev: u64, ino: u64) -> FileIdentity {
        FileIdentity::Inode { dev, ino }
    }

    fn fallback(path: &str) -> FileIdentity {
        FileIdentity::Fallback {
            path: path.to_string(),
            size: 12,
            mtime_ms: Some(1_700_000_000_000),
            hash: crate::hash::sha256_bytes(b"body"),
        }
    }

    #[test]
    fn the_same_file_at_another_path_moved() {
        let verdict = classify_delete(
            &inode(1, 42),
            &[(PathBuf::from("/notes/renamed.md"), inode(1, 42))],
        );
        assert_eq!(
            verdict,
            DeleteVerdict::Moved(PathBuf::from("/notes/renamed.md"))
        );
    }

    #[test]
    fn nothing_carrying_the_identity_was_removed() {
        let verdict = classify_delete(&inode(1, 42), &[]);
        assert_eq!(verdict, DeleteVerdict::Removed);
    }

    #[test]
    fn a_folder_full_of_other_notes_is_still_a_delete() {
        // The candidates are every file left in the folder. None of them is
        // the deleted note, and reading their presence as evidence of a move
        // would leave the tab writing over one of them.
        let verdict = classify_delete(
            &inode(1, 42),
            &[
                (PathBuf::from("/notes/other.md"), inode(1, 43)),
                (PathBuf::from("/notes/third.md"), inode(1, 44)),
            ],
        );
        assert_eq!(verdict, DeleteVerdict::Removed);
    }

    #[test]
    fn the_same_inode_on_another_volume_is_another_file() {
        let verdict = classify_delete(
            &inode(1, 42),
            &[(PathBuf::from("/volumes/backup/note.md"), inode(2, 42))],
        );
        assert_eq!(verdict, DeleteVerdict::Removed);
    }

    #[test]
    fn an_identity_that_cannot_be_read_degrades_rather_than_guessing() {
        // Even with a candidate that matches by description, a fallback
        // identity is never allowed to claim a move.
        let verdict = classify_delete(
            &fallback("/notes/gone.md"),
            &[(PathBuf::from("/notes/gone.md"), fallback("/notes/gone.md"))],
        );
        assert_eq!(verdict, DeleteVerdict::ExternalModification);
    }

    #[test]
    fn a_windows_file_id_recognises_its_file() {
        let before = FileIdentity::Windows {
            volume: 7,
            index: 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef,
        };
        let elsewhere = FileIdentity::Windows {
            volume: 7,
            index: 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef,
        };
        let other = FileIdentity::Windows {
            volume: 7,
            index: 0x0123_4567_89ab_cdef_0123_4567_89ab_cdee,
        };
        let verdict = classify_delete(
            &before,
            &[
                (PathBuf::from(r"C:\notes\other.md"), other),
                (PathBuf::from(r"C:\notes\moved.md"), elsewhere),
            ],
        );
        assert_eq!(
            verdict,
            DeleteVerdict::Moved(PathBuf::from(r"C:\notes\moved.md"))
        );
    }

    #[test]
    fn an_identity_of_one_kind_never_matches_another() {
        let verdict = classify_delete(
            &inode(1, 42),
            &[(
                PathBuf::from("/notes/moved.md"),
                FileIdentity::Windows {
                    volume: 1,
                    index: 42,
                },
            )],
        );
        assert_eq!(verdict, DeleteVerdict::Removed);
    }

    #[test]
    fn the_first_candidate_carrying_the_identity_wins() {
        // Two paths, one file: a hard link is at both, and either answer is
        // true. The caller orders the list, so the answer is theirs.
        let verdict = classify_delete(
            &inode(1, 42),
            &[
                (PathBuf::from("/notes/first.md"), inode(1, 42)),
                (PathBuf::from("/notes/second.md"), inode(1, 42)),
            ],
        );
        assert_eq!(
            verdict,
            DeleteVerdict::Moved(PathBuf::from("/notes/first.md"))
        );
    }

    #[test]
    fn what_the_filesystem_answers_replaces_what_was_recorded() {
        // Another program rewrote the file: sibling temp, rename over the
        // target. Same path, different file. The tab has to hold the id of the
        // file that is there now, or the next rename reads as a delete.
        let sighting = observe_file(Some(inode(1, 42)), Some(inode(1, 77)), true);
        assert_eq!(
            sighting,
            Sighting {
                identity: Some(inode(1, 77)),
                state: SourceState::Present,
            }
        );
    }

    #[test]
    fn a_file_that_will_not_be_described_keeps_the_id_on_record() {
        // An evicted note is not hashed for an id, so the answer is `None`
        // while the file is plainly there. Taking that as the new id would
        // leave the note with nothing to compare a later rename against.
        let sighting = observe_file(Some(inode(1, 42)), None, true);
        assert_eq!(
            sighting,
            Sighting {
                identity: Some(inode(1, 42)),
                state: SourceState::Present,
            }
        );
    }

    #[test]
    fn nothing_at_the_path_is_a_file_that_went() {
        let sighting = observe_file(Some(inode(1, 42)), None, false);
        assert_eq!(
            sighting,
            Sighting {
                identity: None,
                state: SourceState::RemovedOnDisk,
            }
        );
    }

    #[test]
    fn only_a_fallback_is_undurable() {
        assert!(inode(1, 42).is_durable());
        assert!(FileIdentity::Windows {
            volume: 1,
            index: 2
        }
        .is_durable());
        assert!(!fallback("/notes/a.md").is_durable());
    }
}
