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
        /// When the file was created, in nanoseconds since the Unix epoch, as
        /// far as the filesystem will say.
        ///
        /// An inode number is only unique among the files that exist at one
        /// moment: ext4 hands a freed one straight back out, so a file deleted
        /// and another created in the same watcher window can carry the id the
        /// first one had. The birth time is what tells them apart, and it is
        /// the right thing to pair with the inode because a rename does not
        /// touch it (`ctime` does, which is why that is not used here).
        ///
        /// It is not immutable, though. On APFS, setting a file's modification
        /// time earlier than its birth time pulls the birth time back to it, on
        /// the same live inode: `touch -t 202001010000` on a note leaves the
        /// inode alone and reports a birth time twenty-six years earlier than
        /// the one Writ read. So the two known values are compared by order
        /// rather than for equality ([`FileIdentity::is_same_file`]).
        ///
        /// `None` where the filesystem reports no birth time — an ext3 volume,
        /// an ext4 one formatted with 128-byte inodes, a kernel older than the
        /// `statx` that reports it — and then the inode alone is the answer, as
        /// it was before ([`FileIdentity::is_same_file`]).
        birth_ns: Option<u128>,
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

    /// Whether the file seen now is the one `recorded` describes.
    ///
    /// `self` is the candidate read from the filesystem a moment ago and
    /// `recorded` is the id Writ took when it last had the file, and the answer
    /// is directional: the two birth times are compared by order, so flipping
    /// the arguments flips half the verdicts
    /// (`a_backdated_birth_time_and_a_reused_inode_read_opposite_ways`).
    ///
    /// Not `==`, which is exact-value equality and is what a caller comparing
    /// two records of the same read wants ([`identity_to_keep`]). This is the
    /// question a vanished file asks of a candidate, and one field of an inode
    /// identity is allowed to be missing from either side: a volume that
    /// reports no birth time answers `None` for every file on it, so demanding
    /// agreement there would make every move on such a volume read as a
    /// deletion. An unknown birth time on either side leaves the inode as the
    /// whole of the answer.
    ///
    /// Two known birth times, one `dev` and one `ino`, read by where the
    /// candidate's sits against the record's:
    ///
    /// - Equal: the same file, which is what a rename leaves behind.
    /// - Later than the record's: a different file. A freed inode number goes
    ///   to a file created after it was freed, which is after Writ took the
    ///   record, so a reused number arrives with a later birth time on any
    ///   clock that runs forwards.
    /// - Earlier than the record's: the same file. Nothing creates a file in
    ///   the past, so the only way to see this is a birth time that moved on a
    ///   live inode — backdating the modification time below it does that on
    ///   APFS — and reading it as a different file refuses every later save
    ///   over a file that is sitting there.
    ///
    /// Two shapes it cannot tell, both of them a reused inode number reporting
    /// a birth time below the record's. One is a new file that then had its own
    /// birth time pushed back, which needs a deletion, a creation that inherits
    /// the number, and a metadata pass over the result inside one watcher
    /// window. The other is the clock: a birth time is wall clock and not
    /// monotonic, so a step backwards between the record and the creation — NTP
    /// correcting a drifted host, a resume, a restored snapshot — stamps the
    /// new file below the record with nothing having touched it. Both put the
    /// tab on a file nobody opened, which is the failure the birth time exists
    /// to prevent, and both need a second rare event inside the window of the
    /// first.
    pub fn is_same_file(&self, recorded: &Self) -> bool {
        match (self, recorded) {
            (
                Self::Inode { dev, ino, birth_ns },
                Self::Inode {
                    dev: recorded_dev,
                    ino: recorded_ino,
                    birth_ns: recorded_birth_ns,
                },
            ) => {
                dev == recorded_dev
                    && ino == recorded_ino
                    && birth_allows_a_match(*birth_ns, *recorded_birth_ns)
            }
            _ => self == recorded,
        }
    }
}

/// Whether the birth time read from a candidate leaves it able to be the file
/// on record.
///
/// `seen` is the candidate's and `recorded` is the record's, and the order of
/// the two is the whole of the answer: only a file created after the record was
/// taken can be a different file, and only a file whose birth time moved
/// underneath Writ can report an earlier one. An unknown birth time is not
/// evidence of anything, so it rules nothing out.
fn birth_allows_a_match(seen: Option<u128>, recorded: Option<u128>) -> bool {
    match (seen, recorded) {
        (Some(seen), Some(recorded)) => seen <= recorded,
        _ => true,
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
/// elsewhere, so it degrades. A candidate that is the same file
/// ([`FileIdentity::is_same_file`]) is the file, wherever it is. Anything else
/// is a delete — including a folder full of other notes, which is the ordinary
/// case and must not be read as evidence of anything.
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
        if candidate.is_same_file(before) {
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
/// prevent. On a volume that answers for every file — every Unix one — the
/// refusal needs a dataless file or a path holding something that is not a
/// file to reach at all; the case is kept for the volumes and the files where
/// it does.
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

/// Which id a sighting may keep, when reading it was not atomic with recording
/// it.
///
/// Asking the filesystem what a file is costs a syscall, and on a volume with
/// no id to give it costs the whole file, so the read happens outside the lock
/// the record is kept under (ADR-028 §5). That leaves a window: a save can land
/// its own fresher id in it, and the watcher thread would then write the id it
/// read before the save back over the one the save wrote — the stale record
/// this module exists to prevent, arrived at from a race instead.
///
/// `before` is what was on record when the read started and `recorded` is what
/// is on record now. Equal means nothing landed and `seen` is the freshest
/// answer there is. Different means a writer got there first, and a writer that
/// wrote after the read is holding the newer truth, so its value stands.
pub fn identity_to_keep(
    before: Option<&FileIdentity>,
    recorded: Option<&FileIdentity>,
    seen: Option<FileIdentity>,
) -> Option<FileIdentity> {
    if before == recorded {
        seen
    } else {
        recorded.cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inode(dev: u64, ino: u64) -> FileIdentity {
        FileIdentity::Inode {
            dev,
            ino,
            birth_ns: None,
        }
    }

    fn inode_born(dev: u64, ino: u64, birth_ns: u128) -> FileIdentity {
        FileIdentity::Inode {
            dev,
            ino,
            birth_ns: Some(birth_ns),
        }
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
    fn an_inode_handed_back_out_to_a_new_file_is_not_the_old_file() {
        // ext4 reuses a freed inode number at once, so a note deleted and
        // another created in the same window carry one id. The birth time is
        // what says they are two files, and following the id alone would put
        // the tab on a file it has never read.
        let verdict = classify_delete(
            &inode_born(1, 42, 1_700_000_000_000_000_000),
            &[(
                PathBuf::from("/notes/somebody-elses.md"),
                inode_born(1, 42, 1_700_000_000_500_000_000),
            )],
        );
        assert_eq!(verdict, DeleteVerdict::Removed);
    }

    #[test]
    fn a_file_born_when_the_one_on_record_was_is_that_file() {
        // A rename leaves the birth time alone, so the file at the new path
        // answers with the one the record holds.
        let verdict = classify_delete(
            &inode_born(1, 42, 1_700_000_000_000_000_000),
            &[(
                PathBuf::from("/notes/renamed.md"),
                inode_born(1, 42, 1_700_000_000_000_000_000),
            )],
        );
        assert_eq!(
            verdict,
            DeleteVerdict::Moved(PathBuf::from("/notes/renamed.md"))
        );
    }

    #[test]
    fn a_volume_that_reports_no_birth_time_is_answered_on_the_inode_alone() {
        // ext3, and ext4 formatted with 128-byte inodes, report no birth time
        // for any file on them. Reading the missing field as disagreement
        // would turn every move on such a volume into a deletion.
        assert_eq!(
            classify_delete(
                &inode(1, 42),
                &[(PathBuf::from("/notes/renamed.md"), inode(1, 42))]
            ),
            DeleteVerdict::Moved(PathBuf::from("/notes/renamed.md"))
        );
        assert_eq!(
            classify_delete(
                &inode_born(1, 42, 1_700_000_000_000_000_000),
                &[(PathBuf::from("/notes/renamed.md"), inode(1, 42))]
            ),
            DeleteVerdict::Moved(PathBuf::from("/notes/renamed.md")),
            "a birth time on one side only is not evidence of two files"
        );
        assert_eq!(
            classify_delete(
                &inode(1, 42),
                &[(
                    PathBuf::from("/notes/renamed.md"),
                    inode_born(1, 42, 1_700_000_000_000_000_000)
                )]
            ),
            DeleteVerdict::Moved(PathBuf::from("/notes/renamed.md"))
        );
    }

    #[test]
    fn a_different_inode_is_a_different_file_whatever_the_birth_time_says() {
        assert_eq!(
            classify_delete(
                &inode_born(1, 42, 1_700_000_000_000_000_000),
                &[(
                    PathBuf::from("/notes/other.md"),
                    inode_born(1, 43, 1_700_000_000_000_000_000)
                )]
            ),
            DeleteVerdict::Removed
        );
    }

    #[test]
    fn an_unknown_birth_time_reads_the_same_from_either_side() {
        let recorded = inode_born(1, 42, 1_700_000_000_000_000_000);
        let unknown = inode(1, 42);
        assert!(recorded.is_same_file(&unknown));
        assert!(unknown.is_same_file(&recorded));
        assert!(recorded.is_same_file(&recorded));
    }

    #[test]
    fn a_birth_time_earlier_than_the_record_is_the_same_live_inode() {
        // Backdating a file's modification time below its birth time pulls the
        // birth time back to it on APFS, on the inode the tab is editing. Only
        // a file created before the record was taken could report this, and no
        // such file exists, so the earlier value is the record's own file with
        // its metadata rewritten. Reading it as a different file leaves the tab
        // refusing to save over a file that is sitting at its new path.
        let verdict = classify_delete(
            &inode_born(1, 42, 1_700_000_000_000_000_000),
            &[(
                PathBuf::from("/notes/renamed.md"),
                inode_born(1, 42, 1_577_808_000_000_000_000),
            )],
        );
        assert_eq!(
            verdict,
            DeleteVerdict::Moved(PathBuf::from("/notes/renamed.md"))
        );
    }

    #[test]
    fn a_backdated_birth_time_and_a_reused_inode_read_opposite_ways() {
        // The comparison is by order and not by equality, so which side is the
        // record and which is the file seen now decides the verdict. Flipping
        // the two turns a recreated file into a move onto somebody else's note.
        let older = inode_born(1, 42, 1_577_808_000_000_000_000);
        let newer = inode_born(1, 42, 1_700_000_000_000_000_000);
        assert!(
            older.is_same_file(&newer),
            "a birth time that moved back on a live inode is still that file"
        );
        assert!(
            !newer.is_same_file(&older),
            "a file born after the record was taken is the one that inherited the number"
        );
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
    fn an_uncontested_sighting_keeps_what_it_read() {
        let kept = identity_to_keep(Some(&inode(1, 42)), Some(&inode(1, 42)), Some(inode(1, 43)));
        assert_eq!(kept, Some(inode(1, 43)));
    }

    #[test]
    fn a_save_that_landed_during_the_read_keeps_its_own_id() {
        // The watcher read the id, lost the CPU, and a save wrote a fresher
        // one. Writing what the watcher read back over it is the stale record
        // all of this exists to prevent.
        let kept = identity_to_keep(Some(&inode(1, 42)), Some(&inode(1, 99)), Some(inode(1, 42)));
        assert_eq!(kept, Some(inode(1, 99)));
    }

    #[test]
    fn a_first_sighting_of_a_note_with_no_record_keeps_what_it_read() {
        let kept = identity_to_keep(None, None, Some(inode(1, 42)));
        assert_eq!(kept, Some(inode(1, 42)));
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
