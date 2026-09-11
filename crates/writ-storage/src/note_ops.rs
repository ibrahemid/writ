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

use writ_core::notes::guard::{is_not_downloaded, DiskState};
use writ_core::notes::links;
use writ_core::notes::rename::{rewrite_links, Rewrite};
use writ_core::notes::WriteOrigin;

use crate::buffer_store::{dataless_flags, BeforeWrite, DatalessProbe};
use crate::errors::{StorageError, StorageResult};
use crate::guarded::{
    create_note_guarded, guard_rename, write_note_guarded, ConflictPolicy, CreateNote, DiskRead,
    GuardedWrite, TakenName,
};
use crate::notes_index::{indexes_as_note, names_a_note};
use crate::workspace_search::build_walk;

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
/// [`create_note_guarded`]'s: [`StorageError::NoteNameEmpty`] when `stem`
/// holds nothing, [`StorageError::NoteNameTaken`] when the deduped name is on
/// disk anyway, and [`StorageError::Io`] when the folder cannot be created or
/// the file cannot be written.
pub fn create_note(
    notes_root: &Path,
    stem: &str,
    origin: WriteOrigin,
    before_write: BeforeWrite<'_>,
) -> StorageResult<PathBuf> {
    create_note_guarded(
        CreateNote {
            notes_root,
            stem,
            content: "",
            origin,
            on_taken_name: TakenName::Dedupe,
            history: None,
        },
        before_write,
    )
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
    origin: WriteOrigin,
    before_write: BeforeWrite<'_>,
) -> StorageResult<PathBuf> {
    create_note_guarded(
        CreateNote {
            notes_root,
            stem,
            content,
            origin,
            on_taken_name: TakenName::Dedupe,
            history: None,
        },
        before_write,
    )
}

/// Renames a note to `new_stem`, keeping its extension and its folder.
///
/// `new_stem` is already sanitised. The move is refused rather than performed
/// when the name is empty, when the folder already holds that name, or when
/// the file changed since `last_known` — the same guard a save runs
/// ([`guard_rename`]), because a rename that skips it moves a file whose
/// current contents Writ has never seen. Unlike a refused save there is no
/// incoming text to set aside, so no dated copy is written and
/// [`StorageError::SourceChangedOnDisk`] carries `conflict_copy: None`.
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
    origin: WriteOrigin,
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

    guard_rename(from, last_known, origin)?;

    if let Some(stamp) = before_write {
        let bytes = std::fs::read(from).unwrap_or_default();
        stamp(from, &bytes);
        stamp(&to, &bytes);
    }
    std::fs::rename(from, &to)?;
    Ok(to)
}

/// What one file's links did during a rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkRewrite {
    /// The file was rewritten.
    Written,
    /// Its text reaches the renamed note nowhere, and nothing was written.
    NoLink,
    /// A link reaches the renamed note and the file named here, outside the
    /// candidate list, answers to the same name, so nothing was written.
    NameNotUnique(String),
}

/// Every note under `root` a link naming one of `keys` could reach that the
/// index may not hold.
///
/// The walk is the index's own ([`build_walk`]) under the index's own file
/// test ([`indexes_as_note`]), so the two agree about what a note is and where
/// notes are looked for: a folder the index prunes, and anything a
/// `.gitignore` excludes, is scope somebody chose, and a link into it reaches
/// a note no other surface in the app resolves either. A file that is not note
/// text cannot make a name ambiguous — `.git/index` is not a second
/// `index.md`.
///
/// Two gates the index applies are left out, both because they hide a note
/// that is plainly in the folder under the name a link writes. Size: a note
/// over the index's ceiling is still a note on disk. And `is_file`, which is
/// how the index misses a symlinked note rather than a decision it made — so a
/// symlink is followed one step here, though no symlinked folder is descended
/// into. What the target is gets decided from the name alone
/// ([`crate::notes_index::names_a_note`]): the target lies outside the folder
/// this walk was pointed at, and opening it to sniff its bytes is a read of a
/// file nobody named. Whether the symlink is a second note or another name for
/// the renamed one is [`crate::notes_index::index_key`]'s question, which
/// canonicalises.
///
/// `keys` are folded name keys ([`links::candidate_name_keys`]).
pub fn files_named(root: &Path, keys: &[String]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in build_walk(root).build().flatten() {
        let path = entry.path();
        let Some(kind) = entry.file_type() else {
            continue;
        };
        let followed = !kind.is_file();
        if followed && !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
            continue;
        }
        let is_note = match followed {
            true => names_a_note(root, path),
            false => indexes_as_note(root, path),
        };
        if !is_note {
            continue;
        }
        let names = links::candidate_name_keys(&path.to_string_lossy());
        if names.iter().any(|name| keys.contains(name)) {
            found.push(path.to_path_buf());
        }
    }
    found
}

/// Which links a rename counts, and what it writes in their place.
///
/// One value rather than four arguments, because the four are one question:
/// out of everything a link could mean, which of it is the note being renamed.
#[derive(Debug, Clone, Copy)]
pub struct RewriteTarget<'a> {
    /// The renamed note's index key.
    pub target: &'a str,
    /// The name the note is taking.
    pub new_name: &'a str,
    /// Every note a link could reach, as the index spells them.
    pub candidates: &'a [String],
    /// Every file a link could reach that `candidates` does not hold
    /// ([`files_named`]).
    pub unindexed: &'a [String],
}

/// Rewrites the links in `path` that reach the renamed note, so they name it
/// by its new name.
///
/// [`LinkRewrite::Written`] when the file was written, and the two other
/// answers when nothing was. Every refusal comes
/// back as an error naming this file, because a link left pointing at a name
/// no note has any more is exactly what the caller has to be able to say out
/// loud: a propagation that quietly leaves a file behind is worse than one
/// that says which files it left (spec 627).
///
/// The write goes through [`write_note_guarded`] like every other write this
/// crate makes, so the file is stamped before it is replaced and the watcher
/// does not read Writ's own edit as somebody else's.
///
/// Three refusals come with it. A file whose bytes are not on this machine is
/// stopped before the read, because the read is what would pull it down
/// (ADR-028 §5). A file that changed since `last_known` is stopped by the same
/// guard a save runs, and the rewritten text is written beside it as a dated
/// copy, because a propagation that loses a race has to leave its side on disk
/// like every other refusal does (ADR-028 §5). A file the filesystem will not
/// replace — read-only, hard-linked, in a folder that will not take a write —
/// is stopped by the write itself.
///
/// The rewrite is computed before the guard is asked, because the copy a
/// refusal writes is a copy of the rewritten text, and a file no link reaches
/// is answered before either: there is nothing to write and so nothing to
/// lose.
///
/// Which links count is [`rewrite_links`]'s question to answer, note by note,
/// from this file's own key ([`RewriteTarget`]).
///
/// `last_known` is what Writ last saw the file hold, for a file it has looked
/// at; `None` for one it has not, whose "has this changed" has no answer.
/// `dataless` is the eviction probe ([`DatalessProbe`]): `None` asks the
/// filesystem, which is what the app does. `history` is the version store,
/// for the caller that keeps one: a rename propagation rewrites a note
/// nobody has open, so what that note said before is worth keeping.
///
/// # Errors
///
/// [`StorageError::SourceNotDownloaded`], [`StorageError::SourceChangedOnDisk`],
/// [`StorageError::DestinationReadOnly`] and the rest of the write refusals,
/// and [`StorageError::Io`] when the file cannot be read or is not text.
pub fn rewrite_links_in_file(
    path: &Path,
    rename: &RewriteTarget<'_>,
    last_known: Option<DiskState>,
    dataless: DatalessProbe<'_>,
    before_write: BeforeWrite<'_>,
    history: Option<&crate::note_history::NoteHistoryStore>,
) -> StorageResult<LinkRewrite> {
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

    let from = crate::notes_index::index_key(path);
    let rewritten = match rewrite_links(
        &text,
        &from,
        rename.target,
        rename.new_name,
        rename.candidates,
        rename.unindexed,
    ) {
        Rewrite::Rewritten(text) => text,
        Rewrite::NoLink => return Ok(LinkRewrite::NoLink),
        Rewrite::NameNotUnique(other) => return Ok(LinkRewrite::NameNotUnique(other)),
    };

    // The state of the file the rewrite is measured against is read from the
    // bytes the rewrite is built from, not from a second read: two reads of a
    // file something else is writing describe two different files, and the
    // guard would then be answering about the one that was not rewritten.
    let metadata = std::fs::metadata(path).ok();
    let on_disk = DiskState {
        hash: writ_core::hash::sha256_bytes(text.as_bytes()),
        size: metadata
            .as_ref()
            .map(|m| m.len())
            .unwrap_or(text.len() as u64),
        mtime: metadata.as_ref().and_then(|m| m.modified().ok()),
    };

    let keep = history.map(crate::guarded::keep_versions);
    write_note_guarded(
        GuardedWrite {
            target: path,
            bytes: rewritten.as_bytes(),
            last_known,
            on_disk: DiskRead::Read(Some(on_disk)),
            dataless,
            origin: WriteOrigin::RenamePropagation,
            on_conflict: ConflictPolicy::RefuseWithCopy,
            history: keep
                .as_ref()
                .map(|hook| hook as &dyn Fn(crate::guarded::WriteCapture<'_>)),
        },
        before_write,
    )?;
    Ok(LinkRewrite::Written)
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
