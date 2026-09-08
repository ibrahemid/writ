//! The one way a note's file is written.
//!
//! The file is the only copy of a note's text (ADR-028 §1), so a write that
//! lands on a change Writ never read loses that change with nothing left to
//! recover it from. The decision is [`decide_save`]'s and it is asked here,
//! once: every writer in the crate — a save, a new note, a link rewritten
//! after a rename, text a relaunch recovered — comes through
//! [`write_note_guarded`] or [`create_note_guarded`], and the terminal writer
//! they share is private to this module. A writer that inlined the guard
//! instead would be a second answer to the same question, and the copies
//! would drift.
//!
//! Three things travel with a write. Its origin ([`WriteOrigin`]), so the
//! record of what happened to a note says which surface made it. Its conflict
//! policy ([`ConflictPolicy`]), because a refusal that carries text writes it
//! beside the note (ADR-028 §5) and a refusal that carries none has nothing
//! to set aside. And its capture ([`HistoryHook`]), which is where the
//! version store is filled in.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use tracing::warn;
use writ_core::hash::Sha256Digest;
use writ_core::notes::guard::{decide_save, is_not_downloaded, DiskState, SaveDecision};
use writ_core::notes::line_ending::LineEnding;
use writ_core::notes::WriteOrigin;

use crate::atomic::{write_atomic, AtomicWriteError};
use crate::buffer_store::{
    dataless_flags, read_disk_state, taken_names, BeforeWrite, DatalessProbe,
};
use crate::errors::{StorageError, StorageResult};

/// What a refusal does with the text it was handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    /// Write the incoming text beside the note as a dated copy, and name it
    /// in the error. A refused save never ends with the user's text nowhere
    /// (ADR-028 §5).
    RefuseWithCopy,
    /// Refuse and write nothing. For a write that carries no text of its own:
    /// a rename has nothing to set aside.
    RefuseOnly,
}

/// How the guard learns what the file holds now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskRead {
    /// Read the file. What a caller that has not read it passes.
    Fresh,
    /// What the caller read, from the same bytes it built the write from.
    /// A second read of a file something else is writing describes a
    /// different file, and the guard would then be answering about the one
    /// that is not being written.
    Read(Option<DiskState>),
}

/// What a write that landed hands whatever is keeping versions.
pub struct WriteCapture<'a> {
    /// The file that was written.
    pub target: &'a Path,
    /// What asked for the write.
    pub origin: &'a WriteOrigin,
    /// What the file held immediately before, or `None` when there was no
    /// file at the path.
    pub before: Option<&'a [u8]>,
    /// What the file holds now.
    pub after: &'a DiskState,
}

/// The seam a version store is hung on: called once per write that landed,
/// never for a write that was refused and never for one that was not needed.
///
/// `None` for a caller keeping no versions, which is every caller today.
pub type HistoryHook<'a> = Option<&'a dyn Fn(WriteCapture<'_>)>;

/// What a hook that keeps nothing does with a write.
///
/// The default while nothing versions a note's text: the write is handed over
/// and dropped.
pub fn history_hook(_capture: WriteCapture<'_>) {}

/// One write of a note's file, and everything the guard needs to judge it.
pub struct GuardedWrite<'a> {
    /// The file to write.
    pub target: &'a Path,
    /// The bytes to write, line endings and all: what is hashed is what will
    /// land.
    pub bytes: &'a [u8],
    /// What Writ last saw the file hold, or `None` for a file it has not
    /// looked at, whose "has this changed" has no answer.
    pub last_known: Option<DiskState>,
    /// What the file holds now, read here or handed over.
    pub on_disk: DiskRead,
    /// The eviction probe: `None` asks the filesystem, which is what the app
    /// does.
    pub dataless: DatalessProbe<'a>,
    /// What asked for this write.
    pub origin: WriteOrigin,
    /// What a refusal does with `bytes`.
    pub on_conflict: ConflictPolicy,
    /// Where the write is captured, if anywhere.
    pub history: HistoryHook<'a>,
}

/// One new note, minted into the notes folder.
pub struct CreateNote<'a> {
    /// The folder the note is minted in.
    pub notes_root: &'a Path,
    /// The sanitised filename stem
    /// ([`writ_core::notes::note_file_stem`]), before the dedupe.
    pub stem: &'a str,
    /// The text the note starts with, empty for a blank one.
    pub content: &'a str,
    /// What asked for the note.
    pub origin: WriteOrigin,
    /// Where the write is captured, if anywhere.
    pub history: HistoryHook<'a>,
}

/// What became of a write the guard let through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOutcome {
    /// What the file holds afterwards.
    pub disk_state: DiskState,
    /// What the guard decided.
    pub decision: SaveDecision,
    /// The dated copy written beside the note, for a resolution that wrote
    /// one. A refusal reports its copy in
    /// [`StorageError::SourceChangedOnDisk`] instead, because a refusal is
    /// not an outcome.
    pub conflict_copy: Option<PathBuf>,
}

/// Writes `req.bytes` to `req.target` unless doing so would lose a change
/// Writ never read.
///
/// The guard is [`decide_save`] and this is its one call site. A file that
/// already holds the incoming bytes is left alone and reported as written:
/// rewriting identical bytes only moves the modification time and swaps the
/// inode, which a sync client reads as an edit and uploads.
///
/// A file whose bytes are not on this machine is stopped before the compare
/// read, because that read is what would pull it down (ADR-028 §5).
///
/// # Errors
///
/// [`StorageError::SourceChangedOnDisk`] when the file changed under Writ and
/// holds something other than what is being written, naming the dated copy
/// under [`ConflictPolicy::RefuseWithCopy`];
/// [`StorageError::SourceNotDownloaded`] when the file has not finished
/// downloading; [`StorageError::DestinationReadOnly`] and the rest of the
/// write refusals for a destination the filesystem will not replace; and
/// [`StorageError::Io`] when the file cannot be read or written.
pub fn write_note_guarded(
    req: GuardedWrite<'_>,
    before_write: BeforeWrite<'_>,
) -> StorageResult<WriteOutcome> {
    let target = req.target;

    let flags = match req.dataless {
        Some(probe) => probe(target),
        None => dataless_flags(target),
    };
    if is_not_downloaded(flags) {
        return Err(StorageError::SourceNotDownloaded {
            path: target.to_string_lossy().into_owned(),
        });
    }

    let incoming = writ_core::hash::sha256_bytes(req.bytes);
    let on_disk = match req.on_disk {
        DiskRead::Fresh => read_disk_state(target)?,
        DiskRead::Read(state) => state,
    };
    let decision = decide_save(req.last_known.as_ref(), on_disk.as_ref(), incoming);

    // The two decisions that stop a write can only come from a file that is
    // there, so the arms that read one are the only ones that need it. A
    // `None` alongside either could only mean the file went missing between
    // the two reads, which proceeds.
    match (decision, on_disk) {
        (SaveDecision::AlreadyIdentical, Some(state)) => Ok(WriteOutcome {
            disk_state: state,
            decision,
            conflict_copy: None,
        }),
        (SaveDecision::Refuse, Some(state)) => Err(refuse(
            target,
            &state,
            req.bytes,
            req.on_conflict,
            &req.origin,
            before_write,
        )),
        _ => {
            // Read back only for a caller that keeps versions: the pre-write
            // bytes are the one thing the capture cannot recover afterwards,
            // and reading every file twice for a hook nobody supplied would
            // put the cost on every save.
            let before = req.history.and_then(|_| std::fs::read(target).ok());
            write_guarded_by_stamp(target, req.bytes, before_write)?;
            let after = written_state(target, incoming, req.bytes.len() as u64);
            if let Some(hook) = req.history {
                hook(WriteCapture {
                    target,
                    origin: &req.origin,
                    before: before.as_deref(),
                    after: &after,
                });
            }
            Ok(WriteOutcome {
                disk_state: after,
                decision,
                conflict_copy: None,
            })
        }
    }
}

/// Mints a new note file in `req.notes_root` and returns where it landed.
///
/// The name the dedupe picks is checked against the disk before the write.
/// The dedupe reads the folder to learn which names are taken, and a folder it
/// cannot list reads as empty — a folder without read permission, one on a
/// share that answered nothing, a file another process created in between. The
/// write that follows replaces whatever is at the path, so without this check
/// a blind dedupe silently empties a note that was already there. Minting is
/// the one operation that knows its file must not exist yet, so it is the one
/// that can say so.
///
/// # Errors
///
/// [`StorageError::NoteNameEmpty`] when `req.stem` holds nothing,
/// [`StorageError::NoteNameTaken`] when the deduped name is on disk anyway,
/// and [`StorageError::Io`] when the folder cannot be created or the file
/// cannot be written.
pub fn create_note_guarded(
    req: CreateNote<'_>,
    before_write: BeforeWrite<'_>,
) -> StorageResult<PathBuf> {
    let stem = req.stem.trim();
    if stem.is_empty() {
        return Err(StorageError::NoteNameEmpty);
    }
    std::fs::create_dir_all(req.notes_root)?;
    let name = writ_core::notes::dedupe_file_name(
        stem,
        crate::note_ops::NOTE_EXTENSION,
        &taken_names(req.notes_root),
    );
    let path = req.notes_root.join(&name);
    // `symlink_metadata`, so a link left behind by something else counts as
    // taken rather than being followed and written through.
    if path.symlink_metadata().is_ok() {
        return Err(StorageError::NoteNameTaken {
            name,
            folder: req.notes_root.to_path_buf(),
        });
    }
    // A file that does not exist yet has no convention to keep, so a note Writ
    // mints is LF whatever the text handed in carries.
    let content = LineEnding::Lf.apply(req.content);
    write_note_guarded(
        GuardedWrite {
            target: &path,
            bytes: content.as_bytes(),
            last_known: None,
            // The path was just established to hold no file, so there is
            // nothing to read and nothing the guard could refuse.
            on_disk: DiskRead::Read(None),
            dataless: None,
            origin: req.origin,
            on_conflict: ConflictPolicy::RefuseOnly,
            history: req.history,
        },
        before_write,
    )?;
    Ok(path)
}

/// Asks the guard whether `from` may be renamed.
///
/// A rename goes through the same guard a save does, because a rename that
/// skips it moves a file whose current contents Writ has never seen
/// (ADR-028 §5). It carries no text of its own, so the last known digest
/// stands in for the incoming one and a refusal writes nothing beside the
/// note.
///
/// A caller holding no record proceeds: "has this changed since Writ last
/// looked" has no answer for a file Writ has not looked at. A file whose bytes
/// are not on this machine is stopped before the compare read, because that
/// read is what would pull it down.
///
/// # Errors
///
/// [`StorageError::SourceChangedOnDisk`] with no copy beside the note when the
/// file changed under Writ, [`StorageError::SourceNotDownloaded`] when its
/// bytes are not on this machine, and [`StorageError::Io`] when the file
/// cannot be read.
pub fn guard_rename(
    from: &Path,
    last_known: Option<DiskState>,
    origin: WriteOrigin,
) -> StorageResult<()> {
    if last_known.is_some() && is_not_downloaded(dataless_flags(from)) {
        return Err(StorageError::SourceNotDownloaded {
            path: from.to_string_lossy().into_owned(),
        });
    }

    let on_disk = read_disk_state(from)?;
    // Only one answer is read here — whether the guard refuses — so it does
    // not matter which of the two permissive answers a file Writ last saw
    // unchanged comes back with.
    if let (Some(last_known), Some(state)) = (last_known, on_disk) {
        if decide_save(Some(&last_known), Some(&state), last_known.hash) == SaveDecision::Refuse {
            return Err(refuse(
                from,
                &state,
                &[],
                ConflictPolicy::RefuseOnly,
                &origin,
                None,
            ));
        }
    }
    Ok(())
}

/// The error a refused write comes back as, and the copy it leaves behind.
///
/// Under [`ConflictPolicy::RefuseWithCopy`] the incoming text is written
/// beside the note first, so a refusal never ends with it nowhere, and the
/// error names where it went. A copy that could not be written is logged
/// rather than swallowing the refusal: the user still has to be told the save
/// did not land.
fn refuse(
    target: &Path,
    state: &DiskState,
    incoming: &[u8],
    policy: ConflictPolicy,
    origin: &WriteOrigin,
    before_write: BeforeWrite<'_>,
) -> StorageError {
    let conflict_copy = match policy {
        ConflictPolicy::RefuseOnly => None,
        ConflictPolicy::RefuseWithCopy => {
            match write_beside(
                target,
                incoming,
                before_write,
                |stem, now| writ_core::notes::conflict_file_name(stem, "", now),
                Utc::now(),
            ) {
                Ok(written) => Some(written.to_string_lossy().into_owned()),
                Err(e) => {
                    warn!(
                        path = %target.display(),
                        origin = %origin,
                        error = %e,
                        "the copy beside the note could not be written"
                    );
                    None
                }
            }
        }
    };
    StorageError::SourceChangedOnDisk {
        path: target.to_string_lossy().into_owned(),
        disk_hash: writ_core::hash::digest_hex(state.hash),
        conflict_copy,
    }
}

/// Writes `content` beside `note_path` as a dated copy and returns the path
/// written.
///
/// This is what keeps a refused save from ending with the user's text nowhere
/// (ADR-028 §5). The name comes from [`writ_core::notes::conflict_file_name`]
/// and dedupes Finder-style, so two refusals inside the same second produce
/// two files rather than one overwriting the other.
///
/// # Errors
///
/// [`StorageError::Consistency`] when the note has no folder to be written
/// beside, and [`StorageError::Io`] when the copy cannot be written.
pub fn write_conflict_copy(
    note_path: &Path,
    content: &str,
    now: DateTime<Utc>,
    before_write: BeforeWrite<'_>,
) -> StorageResult<PathBuf> {
    write_beside(
        note_path,
        content.as_bytes(),
        before_write,
        |stem, now| writ_core::notes::conflict_file_name(stem, "", now),
        now,
    )
}

/// Writes `content` beside `note_path` as a dated copy the crash snapshot was
/// holding, and returns the path written.
///
/// The relaunch counterpart of [`write_conflict_copy`]: same folder, same
/// dedupe, a name that says where the text came from
/// ([`writ_core::notes::recovered_file_name`]).
///
/// # Errors
///
/// [`StorageError::Consistency`] when the note has no folder to be written
/// beside, and [`StorageError::Io`] when the copy cannot be written.
pub fn write_recovered_copy(
    note_path: &Path,
    content: &str,
    now: DateTime<Utc>,
    before_write: BeforeWrite<'_>,
) -> StorageResult<PathBuf> {
    write_beside(
        note_path,
        content.as_bytes(),
        before_write,
        |stem, now| writ_core::notes::recovered_file_name(stem, "", now),
        now,
    )
}

/// The shared half of both dated copies: name from `name_stem`, dedupe against
/// the folder, stamp, write.
fn write_beside(
    note_path: &Path,
    bytes: &[u8],
    before_write: BeforeWrite<'_>,
    name_stem: impl Fn(&str, DateTime<Utc>) -> String,
    now: DateTime<Utc>,
) -> StorageResult<PathBuf> {
    let dir = note_path
        .parent()
        .ok_or_else(|| StorageError::Consistency {
            message: format!("{} has no folder to be written beside", note_path.display()),
        })?;
    let stem = note_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = note_path
        .extension()
        .map(|ext| ext.to_string_lossy().into_owned())
        .unwrap_or_default();

    let name =
        writ_core::notes::dedupe_file_name(&name_stem(&stem, now), &extension, &taken_names(dir));
    let target = dir.join(name);
    write_guarded_by_stamp(&target, bytes, before_write)?;
    Ok(target)
}

/// Refuses, stamps, then writes, in that order.
///
/// Every write this crate performs goes through here, because a write the
/// caller has not been told about first is a write its watcher reads as
/// somebody else's edit. [`write_atomic`] has this one call site so that no
/// future write can skip the stamp by reaching past it, and this function has
/// its own single caller inside this module so that no future write can skip
/// the guard either.
///
/// The destination is asked whether it can be replaced before the stamp
/// rather than after: an ignore entry for a write that never happens is one
/// the watcher spends on the next real change carrying those bytes.
/// [`write_atomic`] asks again, so a caller reaching for it directly is
/// covered too.
pub(crate) fn write_guarded_by_stamp(
    target: &Path,
    bytes: &[u8],
    before_write: BeforeWrite<'_>,
) -> StorageResult<()> {
    crate::atomic::refuse_unreplaceable_destination(target)
        .map_err(|e| refusal_as_storage_error(target, e))?;
    if let Some(stamp) = before_write {
        stamp(target, bytes);
    }
    write_atomic(target, bytes).map_err(|e| refusal_as_storage_error(target, e))
}

/// The state of a file just written, without reading it back.
///
/// The digest and the length are what was written; only the modification time
/// has to come from the filesystem, and a file whose metadata cannot be read
/// records none rather than failing a save that already landed.
fn written_state(path: &Path, hash: Sha256Digest, size: u64) -> DiskState {
    DiskState {
        hash,
        size,
        mtime: std::fs::metadata(path).ok().and_then(|m| m.modified().ok()),
    }
}

/// Names the file a refused write was aimed at.
///
/// [`AtomicWriteError`] knows what it found and nothing about where; the
/// error the editor reads has to carry the path, because it is what the
/// message names.
fn refusal_as_storage_error(target: &Path, error: AtomicWriteError) -> StorageError {
    match error {
        AtomicWriteError::HardLinked { links } => StorageError::HardLinkedDestination {
            path: target.display().to_string(),
            links,
        },
        AtomicWriteError::ReadOnly => StorageError::DestinationReadOnly {
            path: target.display().to_string(),
        },
        AtomicWriteError::FolderNotWritable => StorageError::DestinationFolderNotWritable {
            path: target.display().to_string(),
        },
        AtomicWriteError::Io(e) => StorageError::Io(e),
    }
}
