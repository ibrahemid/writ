//! Creating, renaming, trashing and copying the files notes live in.
//!
//! A note is managed from inside Writ and the file follows (ADR-028 §3), so
//! every one of these operations moves a real file in the notes folder. The
//! policy half — what a title becomes, how a collision dedupes — is
//! [`writ_core::notes`]; this is the half that touches the disk.
//!
//! Two rules hold across the module. A rename goes through the same guard a
//! save does, because a rename that skips it clobbers a file another process
//! created between the check and the move (ADR-028 §5). And a note is never
//! unlinked: [`trash_note`] hands it to the operating system's trash, so a
//! deletion is recoverable by the means the user already knows.

use std::path::{Path, PathBuf};

use writ_core::notes::guard::{decide_save, is_not_downloaded, DiskState, SaveDecision};
use writ_core::notes::line_ending::LineEnding;
use writ_core::notes::links::WikilinkTarget;
use writ_core::notes::rename::rewrite_links;

use crate::buffer_store::{
    dataless_flags, read_disk_state, taken_names, write_guarded_by_stamp, BeforeWrite,
    DatalessProbe,
};
use crate::errors::{StorageError, StorageResult};

/// Extension every note Writ mints carries.
pub const NOTE_EXTENSION: &str = "md";

/// Creates an empty note file in `notes_root`, named from `stem`.
///
/// `stem` is already sanitised ([`writ_core::notes::note_file_stem`]); the
/// name is deduped Finder-style against what the folder holds. The file exists
/// on return, which is the whole point: a new note is visible in Finder before
/// anything else happens, not on the first keystroke and not at quit
/// (ADR-028 §3).
///
/// # Errors
///
/// [`StorageError::NoteNameEmpty`] when `stem` holds nothing, and
/// [`StorageError::Io`] when the folder cannot be created or the file cannot
/// be written.
pub fn create_note(
    notes_root: &Path,
    stem: &str,
    before_write: BeforeWrite<'_>,
) -> StorageResult<PathBuf> {
    write_new_note(notes_root, stem, "", before_write)
}

/// Writes `content` into `notes_root` as a new note named from `stem`,
/// leaving the file it came from untouched.
///
/// This is `Save a Copy…`: a note opened from somewhere else earns a place in
/// the notes folder without moving, so the original stays exactly where its
/// owner put it.
///
/// # Errors
///
/// The same as [`create_note`].
pub fn save_copy(
    notes_root: &Path,
    stem: &str,
    content: &str,
    before_write: BeforeWrite<'_>,
) -> StorageResult<PathBuf> {
    write_new_note(notes_root, stem, content, before_write)
}

/// The shared half of both: dedupe against the folder, stamp, write.
///
/// The name the dedupe picks is checked against the disk before the write. The
/// dedupe reads the folder to learn which names are taken, and a folder it
/// cannot list reads as empty — a folder without read permission, one on a
/// share that answered nothing, a file another process created in between. The
/// write that follows replaces whatever is at the path, so without this check a
/// blind dedupe silently empties a note that was already there. Minting is the
/// one operation that knows its file must not exist yet, so it is the one that
/// can say so.
fn write_new_note(
    notes_root: &Path,
    stem: &str,
    content: &str,
    before_write: BeforeWrite<'_>,
) -> StorageResult<PathBuf> {
    let stem = stem.trim();
    if stem.is_empty() {
        return Err(StorageError::NoteNameEmpty);
    }
    std::fs::create_dir_all(notes_root)?;
    let name = writ_core::notes::dedupe_file_name(stem, NOTE_EXTENSION, &taken_names(notes_root));
    let path = notes_root.join(&name);
    // `symlink_metadata`, so a link left behind by something else counts as
    // taken rather than being followed and written through.
    if path.symlink_metadata().is_ok() {
        return Err(StorageError::NoteNameTaken {
            name,
            folder: notes_root.to_path_buf(),
        });
    }
    // A file that does not exist yet has no convention to keep, so a note Writ
    // mints is LF whatever the text handed in carries.
    let content = LineEnding::Lf.apply(content);
    write_guarded_by_stamp(&path, content.as_bytes(), before_write)?;
    Ok(path)
}

/// Renames a note to `new_stem`, keeping its extension and its folder.
///
/// `new_stem` is already sanitised. The move is refused rather than performed
/// when the name is empty, when the folder already holds that name, or when
/// the file changed since `last_known` — the same guard a save runs
/// ([`decide_save`]), because a rename that skips it moves a file whose
/// current contents Writ has never seen. Unlike a refused save there is no
/// incoming text to set aside, so no dated copy is written and
/// [`StorageError::SourceChangedOnDisk`] carries `conflict_copy: None`.
///
/// A file whose bytes are not on this machine is stopped before the compare
/// read, because that read is what would pull it down (ADR-028 §5). A caller
/// holding no record proceeds: "has this changed since Writ last looked" has
/// no answer for a file Writ has not looked at.
///
/// `before_write` stamps both the old and the new path before the move. One
/// rename reaches the watcher as a delete of the first plus a create of the
/// second, and an unstamped pair reads as somebody else deleting a note and
/// somebody else adding one.
///
/// The move itself is [`std::fs::rename`], which is atomic within one volume.
/// A note inside the notes folder is renamed inside that folder, so the
/// cross-volume case this does not cover cannot arise here; a caller that
/// wants to move a note to another volume needs a copy-then-trash, which is
/// [`save_copy`] plus [`trash_note`].
///
/// # Errors
///
/// [`StorageError::NoteNameEmpty`] for a name with nothing in it,
/// [`StorageError::NoteNameTaken`] naming the file already there,
/// [`StorageError::SourceChangedOnDisk`] when the file changed under Writ,
/// [`StorageError::SourceNotDownloaded`] when its bytes are not on this
/// machine, and [`StorageError::Io`] when the move fails.
pub fn rename_note(
    from: &Path,
    new_stem: &str,
    last_known: Option<DiskState>,
    before_write: BeforeWrite<'_>,
) -> StorageResult<PathBuf> {
    let new_stem = new_stem.trim();
    if new_stem.is_empty() {
        return Err(StorageError::NoteNameEmpty);
    }

    let folder = from.parent().ok_or_else(|| StorageError::Consistency {
        message: format!("{} has no folder to be renamed inside", from.display()),
    })?;
    let extension = from
        .extension()
        .map(|ext| ext.to_string_lossy().into_owned())
        .unwrap_or_else(|| NOTE_EXTENSION.to_string());
    let name = join_name(new_stem, &extension);
    let to = folder.join(&name);

    if is_same_file(from, &to) {
        return Ok(to);
    }
    if to.exists() {
        return Err(StorageError::NoteNameTaken {
            name,
            folder: folder.to_path_buf(),
        });
    }

    if last_known.is_some() && is_not_downloaded(dataless_flags(from)) {
        return Err(StorageError::SourceNotDownloaded {
            path: from.to_string_lossy().into_owned(),
        });
    }

    let on_disk = read_disk_state(from)?;
    // The rename carries no text of its own, so there is no incoming hash to
    // compare against; the last known digest stands in for it. Only one answer
    // is read here — whether the guard refuses — so it does not matter which
    // of the two permissive answers a file Writ last saw unchanged comes back
    // with.
    if let (Some(last_known), Some(state)) = (last_known, on_disk) {
        if decide_save(Some(&last_known), Some(&state), last_known.hash) == SaveDecision::Refuse {
            return Err(StorageError::SourceChangedOnDisk {
                path: from.to_string_lossy().into_owned(),
                disk_hash: writ_core::hash::digest_hex(state.hash),
                conflict_copy: None,
            });
        }
    }

    if let Some(stamp) = before_write {
        let bytes = std::fs::read(from).unwrap_or_default();
        stamp(from, &bytes);
        stamp(&to, &bytes);
    }
    std::fs::rename(from, &to)?;
    Ok(to)
}

/// Rewrites the links in `path` that name `old`, so they name `new_name`.
///
/// `Ok(true)` when the file was written, `Ok(false)` when its text names the
/// renamed note nowhere and there was nothing to write. Every refusal comes
/// back as an error naming this file, because a link left pointing at a name
/// no note has any more is exactly what the caller has to be able to say out
/// loud: a propagation that quietly leaves a file behind is worse than one
/// that says which files it left (spec 627).
///
/// The write goes through [`write_guarded_by_stamp`] like every other write
/// this crate makes, so the file is stamped before it is replaced and the
/// watcher does not read Writ's own edit as somebody else's.
///
/// Three refusals come before the write. A file whose bytes are not on this
/// machine is stopped before the read, because the read is what would pull it
/// down (ADR-028 §5). A file that changed since `last_known` is stopped by the
/// same guard a save runs, because rewriting it would carry text Writ never
/// saw. A file the filesystem will not replace — read-only, hard-linked, in a
/// folder that will not take a write — is stopped by the write itself.
///
/// `last_known` is what Writ last saw the file hold, for a file it has looked
/// at; `None` for one it has not, whose "has this changed" has no answer.
/// `dataless` is the eviction probe ([`DatalessProbe`]): `None` asks the
/// filesystem, which is what the app does.
///
/// # Errors
///
/// [`StorageError::SourceNotDownloaded`], [`StorageError::SourceChangedOnDisk`],
/// [`StorageError::DestinationReadOnly`] and the rest of the write refusals,
/// and [`StorageError::Io`] when the file cannot be read or is not text.
pub fn rewrite_links_in_file(
    path: &Path,
    old: &WikilinkTarget,
    new_name: &str,
    last_known: Option<DiskState>,
    dataless: DatalessProbe<'_>,
    before_write: BeforeWrite<'_>,
) -> StorageResult<bool> {
    let flags = match dataless {
        Some(probe) => probe(path),
        None => dataless_flags(path),
    };
    if is_not_downloaded(flags) {
        return Err(StorageError::SourceNotDownloaded {
            path: path.to_string_lossy().into_owned(),
        });
    }

    let bytes = std::fs::read(path)?;
    let text = String::from_utf8(bytes).map_err(|_| {
        StorageError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{} is not text", path.display()),
        ))
    })?;

    // The state of the file the rewrite is measured against is read from the
    // bytes the rewrite is built from, not from a second read: two reads of a
    // file something else is writing describe two different files, and the
    // guard would then be answering about the one that was not rewritten.
    if let Some(last_known) = last_known {
        let metadata = std::fs::metadata(path).ok();
        let state = DiskState {
            hash: writ_core::hash::sha256_bytes(text.as_bytes()),
            size: metadata
                .as_ref()
                .map(|m| m.len())
                .unwrap_or(text.len() as u64),
            mtime: metadata.as_ref().and_then(|m| m.modified().ok()),
        };
        if decide_save(Some(&last_known), Some(&state), last_known.hash) == SaveDecision::Refuse {
            return Err(StorageError::SourceChangedOnDisk {
                path: path.to_string_lossy().into_owned(),
                disk_hash: writ_core::hash::digest_hex(state.hash),
                conflict_copy: None,
            });
        }
    }

    let Some(rewritten) = rewrite_links(&text, old, new_name) else {
        return Ok(false);
    };
    write_guarded_by_stamp(path, rewritten.as_bytes(), before_write)?;
    Ok(true)
}

/// Moves a note to the operating system's trash.
///
/// Never an unlink: a note the user deleted has to be recoverable the way
/// every other file they delete is, from the Trash, the Recycle Bin or the
/// freedesktop trash folder. The caller deletes the row only after this
/// returns, so a failure here leaves the note both on disk and in Writ.
///
/// # Errors
///
/// [`StorageError::NoteTrash`] when the platform will not take the file, which
/// a file on a volume with no trash and a file already gone both produce.
pub fn trash_note(path: &Path) -> StorageResult<()> {
    trash_context()
        .delete(path)
        .map_err(|error| StorageError::NoteTrash {
            path: path.to_path_buf(),
            message: error.to_string(),
        })
}

/// The trash the delete goes through, configured for the platform.
///
/// On macOS the crate defaults to driving Finder over `osascript`, which sends
/// an Apple Event. A hardened, notarized build has no
/// `com.apple.security.automation.apple-events` entitlement, so that route
/// either prompts the user for automation permission or fails outright, and
/// the note stays where it is. `NSFileManager` needs no permission and is the
/// only route a shipped build can rely on. The cost is that the Finder's
/// "Put Back" entry may be missing on some systems; the file is in the Trash
/// either way, which is what the promise is.
#[cfg(target_os = "macos")]
fn trash_context() -> trash::TrashContext {
    use trash::macos::{DeleteMethod, TrashContextExtMacos};
    let mut context = trash::TrashContext::new();
    context.set_delete_method(DeleteMethod::NsFileManager);
    context
}

/// [`trash_context`] where the platform has one route and no choice to make.
#[cfg(not(target_os = "macos"))]
fn trash_context() -> trash::TrashContext {
    trash::TrashContext::new()
}

/// Whether both paths name the same file, which a rename that only changes
/// case does on a case-insensitive filesystem.
///
/// Comparing the canonical forms is what tells `Notes.md` renamed to
/// `notes.md` apart from a genuine collision: APFS and NTFS report the
/// destination as existing in both cases.
#[cfg(unix)]
fn is_same_file(from: &Path, to: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    if from == to {
        return true;
    }
    match (std::fs::metadata(from), std::fs::metadata(to)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// [`is_same_file`] where there is no inode to compare. `canonicalize` returns
/// the name the filesystem holds rather than the one that was asked for, so
/// two spellings of one file resolve to the same string.
#[cfg(not(unix))]
fn is_same_file(from: &Path, to: &Path) -> bool {
    if from == to {
        return true;
    }
    match (std::fs::canonicalize(from), std::fs::canonicalize(to)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn join_name(stem: &str, extension: &str) -> String {
    if extension.is_empty() {
        stem.to_string()
    } else {
        format!("{stem}.{extension}")
    }
}
