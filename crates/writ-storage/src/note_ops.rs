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

use chrono::{DateTime, Utc};
use writ_core::config::FileExtension;

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
use crate::notes_index::{build_notes_walk, indexes_as_note, names_a_note};

/// Creates an empty note file in `notes_root`, named from `stem` and
/// `extension`.
///
/// `stem` is already sanitised ([`writ_core::notes::note_file_stem`]); the
/// name is deduped by counter against what the folder holds. `extension` is
/// the caller's configured one ([`writ_core::config::FilesConfig`]) — this
/// module reads no config, so every mint is told which format it is making
/// (ADR-041 §2). The file exists on return, which is the whole point: a new
/// note is visible in Finder before anything else happens, not on the first
/// keystroke and not at quit (ADR-028 §3).
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
    extension: &str,
    origin: WriteOrigin,
    before_write: BeforeWrite<'_>,
) -> StorageResult<PathBuf> {
    create_note_guarded(
        CreateNote {
            notes_root,
            stem,
            extension,
            content: "",
            origin,
            on_taken_name: TakenName::Dedupe,
            history: None,
            lock: None,
        },
        before_write,
    )
}

/// Writes `content` into `notes_root` as a new note named from `stem` and
/// `extension`, leaving the file it came from untouched.
///
/// This is `Save a Copy…`: a note opened from somewhere else earns a place in
/// the notes folder without moving, so the original stays exactly where its
/// owner put it. The copy carries the original's own extension, so a copy of
/// a `.txt` is a `.txt` and the copy reads the way the file it came from did.
///
/// # Errors
///
/// The same as [`create_note`].
pub fn save_copy(
    notes_root: &Path,
    stem: &str,
    extension: &str,
    content: &str,
    origin: WriteOrigin,
    before_write: BeforeWrite<'_>,
) -> StorageResult<PathBuf> {
    create_note_guarded(
        CreateNote {
            notes_root,
            stem,
            extension,
            content,
            origin,
            on_taken_name: TakenName::Dedupe,
            history: None,
            lock: None,
        },
        before_write,
    )
}

/// Renames a note to `new_stem`, in its own folder.
///
/// A name that spells its own format takes it: `Groceries.md` on a `.txt`
/// file is `Groceries.md`, because a name ending in a text extension
/// ([`writ_core::notes::explicit_extension`]) answers the format question the
/// config otherwise answers. Every other name keeps the file's own extension,
/// and a file with no extension keeps none: `Draft.rtf` spells no format Writ
/// mints, so it is a stem and a `.txt` file renamed to it is `Draft.rtf.txt`.
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
/// The move itself is [`rename_file`]: a rename that refuses to replace a
/// file already at the new name, atomic within one volume. A note inside the
/// notes folder is renamed inside that folder, so the cross-volume case this
/// does not cover cannot arise here; a caller that wants to move a note to
/// another volume needs a copy-then-trash, which is [`save_copy`] plus
/// [`trash_note`].
///
/// A name that differs from the current one only by case or by Unicode
/// normalisation is a rename like any other. APFS and NTFS hold `Notes.md`
/// and `notes.md` as one file, so the move is made in place rather than
/// refused as a taken name, and the new spelling reaches the disk.
///
/// The stem is [`writ_core::notes::note_stem_for`] the extension it ends up
/// with, so a Markdown note renamed to `C# notes` is `C notes.md`.
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
    prepare_rename(from, new_stem, last_known, origin, before_write)?.commit()
}

/// [`rename_note`] up to the move: the name checks, the guard and the stamps,
/// each of which reads the whole file.
///
/// A caller that keeps the move atomic with a lock it must not hold across a
/// file's bytes runs this before taking the lock and
/// [`PreparedRename::commit`] under it (ADR-047).
///
/// # Errors
///
/// Those of [`rename_note`], except a failed move.
pub fn prepare_rename(
    from: &Path,
    new_stem: &str,
    last_known: Option<DiskState>,
    origin: WriteOrigin,
    before_write: BeforeWrite<'_>,
) -> StorageResult<PreparedRename> {
    prepare_rename_probed(
        from,
        new_stem,
        last_known,
        origin,
        before_write,
        &dataless_flags,
    )
}

/// A rename that passed its checks and stamped both names, before the move.
#[must_use = "a prepared rename moves nothing until it is committed"]
#[derive(Debug)]
pub struct PreparedRename {
    from: PathBuf,
    to: PathBuf,
    /// `false` when the new name is the one the file already has.
    moves: bool,
}

impl PreparedRename {
    /// The path the file has once the rename is committed.
    pub fn to(&self) -> &Path {
        &self.to
    }

    /// Moves the file. Only the two names are touched, never the bytes.
    ///
    /// The move refuses a file that arrived at the new name between the checks
    /// and the move ([`rename_file`]).
    ///
    /// # Errors
    ///
    /// [`StorageError::NoteNameTaken`] when the new name was taken since the
    /// checks, and [`StorageError::Io`] when the move fails.
    pub fn commit(self) -> StorageResult<PathBuf> {
        if !self.moves {
            return Ok(self.to);
        }
        rename_file(&self.from, &self.to)?;
        Ok(self.to)
    }
}

/// [`prepare_rename`], with the filesystem flags read through `dataless`.
///
/// A file whose bytes are not on this machine is moved unread and unstamped:
/// the stamp fingerprints the bytes, and reading them is the download the move
/// must not cause. The watcher then reports the move as a change it cannot
/// fingerprint, which the index answers without reading either.
fn prepare_rename_probed(
    from: &Path,
    new_stem: &str,
    last_known: Option<DiskState>,
    origin: WriteOrigin,
    before_write: BeforeWrite<'_>,
    dataless: &dyn Fn(&Path) -> Option<u32>,
) -> StorageResult<PreparedRename> {
    let new_stem = new_stem.trim();
    if new_stem.is_empty() {
        return Err(StorageError::NoteNameEmpty);
    }

    let folder = from.parent().ok_or_else(|| StorageError::Consistency {
        message: format!("{} has no folder to be renamed inside", from.display()),
    })?;
    let (new_stem, extension) = match writ_core::notes::explicit_extension(new_stem) {
        Some((stem, extension)) => (stem.trim(), extension.to_string()),
        None => (
            new_stem,
            from.extension()
                .map(|ext| ext.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
    };
    let new_stem =
        writ_core::notes::note_stem_for(new_stem, &extension).ok_or(StorageError::NoteNameEmpty)?;
    let name = join_name(&new_stem, &extension);
    let to = folder.join(&name);

    if is_same_file(from, &to) {
        if from.file_name() == to.file_name() {
            return Ok(PreparedRename {
                from: from.to_path_buf(),
                to,
                moves: false,
            });
        }
        // The same file under a spelling that is not a case or normalisation
        // of its own name is a second hard link to it, which is a taken name.
        if !is_respelling(from, &to) {
            return Err(StorageError::NoteNameTaken {
                name,
                folder: folder.to_path_buf(),
            });
        }
    } else if to.symlink_metadata().is_ok() {
        return Err(StorageError::NoteNameTaken {
            name,
            folder: folder.to_path_buf(),
        });
    }

    guard_rename(from, last_known, origin)?;

    if let Some(stamp) = before_write {
        if !is_not_downloaded(dataless(from)) {
            let bytes = std::fs::read(from).unwrap_or_default();
            stamp(from, &bytes);
            stamp(&to, &bytes);
        }
    }
    Ok(PreparedRename {
        from: from.to_path_buf(),
        to,
        moves: true,
    })
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
/// The walk is the index's own ([`build_notes_walk`]) under the index's own
/// file test ([`indexes_as_note`]), so the two agree about what a note is and
/// where notes are looked for: a folder the index skips (`.git`, `.obsidian`,
/// `.trash`, `.stfolder`, `.stversions`) holds nothing a link reaches, and a
/// note under `vendor/` or one a `.gitignore` names is as much a note here as
/// it is to the index. A file that is not note text cannot make a name
/// ambiguous — `.git/index` is not a second `index.md`.
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
/// `keys` are folded name keys ([`links::candidate_name_keys`]). The name is
/// compared before the note test, which can read a file's bytes, so the walk
/// reads nothing but the folder listing for a file of any other name.
pub fn files_named(root: &Path, keys: &[String]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in build_notes_walk(root).build().flatten() {
        let path = entry.path();
        let names = links::candidate_name_keys(&path.to_string_lossy());
        if !names.iter().any(|name| keys.contains(name)) {
            continue;
        }
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
        if is_note {
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
            lock: None,
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

/// Puts a note Writ moved to the Trash back at `path`, from the bytes the file
/// held when it went.
///
/// The bytes are written as they were, encoding and line endings included.
/// The Trash keeps its own copy: taking that one back needs the place the
/// platform moved it to, which the `trash` crate does not report on macOS.
/// Nothing at `path` is ever replaced.
///
/// # Errors
///
/// [`StorageError::NoteNameTaken`] when something is at `path` now, and
/// [`StorageError::Io`] when the folder cannot be created or the file cannot
/// be written.
pub fn restore_trashed(
    path: &Path,
    bytes: &[u8],
    before_write: BeforeWrite<'_>,
) -> StorageResult<()> {
    let folder = path.parent().ok_or_else(|| {
        StorageError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a file path with no folder",
        ))
    })?;
    let taken = || StorageError::NoteNameTaken {
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        folder: folder.to_path_buf(),
    };
    std::fs::create_dir_all(folder)?;
    // `symlink_metadata`, so a link left at the path counts as taken rather
    // than being followed and written through.
    if path.symlink_metadata().is_ok() {
        return Err(taken());
    }
    write_note_guarded(
        GuardedWrite {
            target: path,
            bytes,
            last_known: None,
            on_disk: DiskRead::Read(None),
            dataless: None,
            origin: WriteOrigin::Editor,
            on_conflict: ConflictPolicy::RefuseOnly,
            history: None,
            lock: None,
        },
        before_write,
    )
    .map_err(|error| match error {
        StorageError::SourceChangedOnDisk { .. } => taken(),
        other => other,
    })?;
    Ok(())
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

/// Moves the file at `from` to `to` without replacing a file already at `to`.
///
/// `renamex_np(RENAME_EXCL)` on macOS, `renameat2(RENAME_NOREPLACE)` on Linux
/// and `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING` on Windows: the
/// filesystem itself refuses the move when the name is taken, so a file
/// another process creates there between a check and the move is never
/// replaced. A filesystem that does not support the flag gets a check and a
/// plain rename.
///
/// A `to` that is `from` spelled with another case or normalisation
/// ([`is_respelling`]) names the same file on APFS and NTFS, so the new
/// spelling is written in place: by a plain rename where the filesystem
/// renames a file onto itself, and through a temporary name on Windows.
///
/// # Errors
///
/// [`StorageError::NoteNameTaken`] when `to` holds another file, and
/// [`StorageError::Io`] when the move fails.
pub fn rename_file(from: &Path, to: &Path) -> StorageResult<()> {
    let taken = || StorageError::NoteNameTaken {
        name: to
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        folder: to.parent().map(Path::to_path_buf).unwrap_or_default(),
    };
    if is_same_file(from, to) && from != to && is_respelling(from, to) {
        return respell(from, to).map_err(StorageError::from);
    }
    match rename_no_replace(from, to) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(taken()),
        Err(error) => Err(error.into()),
    }
}

/// Whether `to` is `from` in the same folder with its name spelled in another
/// case or another Unicode normalisation.
fn is_respelling(from: &Path, to: &Path) -> bool {
    match (from.file_name(), to.file_name()) {
        (Some(a), Some(b)) => {
            from.parent() == to.parent()
                && links::name_key(&a.to_string_lossy()) == links::name_key(&b.to_string_lossy())
        }
        _ => false,
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub(crate) fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let from_c = std::ffi::CString::new(from.as_os_str().as_bytes())?;
    let to_c = std::ffi::CString::new(to.as_os_str().as_bytes())?;
    // SAFETY: both arguments are NUL-terminated strings that outlive the call.
    let status = unsafe { libc::renamex_np(from_c.as_ptr(), to_c.as_ptr(), libc::RENAME_EXCL) };
    if status == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    // exFAT, FAT and some network volumes do not take the flag.
    match error.raw_os_error() {
        Some(libc::ENOTSUP | libc::EINVAL) => checked_rename(from, to),
        _ => Err(error),
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[allow(unsafe_code)]
pub(crate) fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let from_c = std::ffi::CString::new(from.as_os_str().as_bytes())?;
    let to_c = std::ffi::CString::new(to.as_os_str().as_bytes())?;
    // SAFETY: both paths are NUL-terminated strings that outlive the call, and
    // AT_FDCWD resolves them as given.
    let status = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from_c.as_ptr(),
            libc::AT_FDCWD,
            to_c.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if status == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::EINVAL | libc::ENOSYS) => checked_rename(from, to),
        _ => Err(error),
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
pub(crate) fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVE_FILE_FLAGS};
    let wide = |path: &Path| -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let (from, to) = (wide(from), wide(to));
    // SAFETY: both buffers are NUL-terminated UTF-16 that outlive the call.
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVE_FILE_FLAGS(0),
        )
    }
    .map_err(|error| std::io::Error::from_raw_os_error(error.code().0 & 0xFFFF))
}

#[cfg(not(any(
    target_os = "macos",
    all(target_os = "linux", target_env = "gnu"),
    windows
)))]
pub(crate) fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    checked_rename(from, to)
}

/// A check, then a plain rename, for a filesystem with no exclusive rename.
#[cfg(not(windows))]
fn checked_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    if to.symlink_metadata().is_ok() {
        return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists));
    }
    std::fs::rename(from, to)
}

/// Writes the new spelling of a file's own name.
#[cfg(not(windows))]
fn respell(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

/// Writes the new spelling of a file's own name through a temporary name, so
/// the directory entry is replaced rather than renamed onto itself.
#[cfg(windows)]
fn respell(from: &Path, to: &Path) -> std::io::Result<()> {
    let folder = to.parent().unwrap_or_else(|| Path::new("."));
    let through = folder.join(format!(".writ-rename-{}", uuid::Uuid::new_v4().simple()));
    rename_no_replace(from, &through)?;
    if let Err(error) = rename_no_replace(&through, to) {
        let _ = rename_no_replace(&through, from);
        return Err(error);
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use writ_core::notes::guard::SF_DATALESS;

    /// [`rename_note`], with the filesystem flags read through `dataless` so
    /// a test can stand in for a file whose bytes are not on this machine.
    fn rename_note_probed(
        from: &Path,
        new_stem: &str,
        last_known: Option<DiskState>,
        origin: WriteOrigin,
        before_write: BeforeWrite<'_>,
        dataless: &dyn Fn(&Path) -> Option<u32>,
    ) -> StorageResult<PathBuf> {
        prepare_rename_probed(from, new_stem, last_known, origin, before_write, dataless)?.commit()
    }

    #[test]
    fn a_placeholder_is_renamed_without_being_stamped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("Evicted.md");
        std::fs::write(&from, "text the provider holds").expect("seed");
        let stamped = RefCell::new(Vec::new());
        let stamp = |path: &Path, _: &[u8]| stamped.borrow_mut().push(path.to_path_buf());

        let to = rename_note_probed(
            &from,
            "Moved",
            None,
            WriteOrigin::Editor,
            Some(&stamp),
            &|_| Some(SF_DATALESS),
        )
        .expect("rename");

        assert_eq!(to, dir.path().join("Moved.md"));
        assert!(to.exists() && !from.exists());
        assert!(stamped.borrow().is_empty(), "stamping reads the bytes");
    }

    #[test]
    fn a_downloaded_file_is_stamped_at_both_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("Here.md");
        std::fs::write(&from, "local text").expect("seed");
        let stamped = RefCell::new(Vec::new());
        let stamp = |path: &Path, _: &[u8]| stamped.borrow_mut().push(path.to_path_buf());

        let to = rename_note_probed(
            &from,
            "There",
            None,
            WriteOrigin::Editor,
            Some(&stamp),
            &|_| None,
        )
        .expect("rename");

        assert_eq!(*stamped.borrow(), vec![from, to]);
    }
}

/// Where Today's File is, or where it would be minted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TodaysFileAt {
    /// The dated file, in whichever format the day already has it.
    Existing(PathBuf),
    /// No file for the day yet: the path the configured format mints.
    Missing(PathBuf),
}

/// Finds Today's File in `notes_root` without opening or minting it.
///
/// Both formats the dated name may be written in are looked for, the
/// configured one first ([`writ_core::startup::dated_note_names`]).
///
/// # Errors
///
/// [`StorageError::NoteNameTaken`] when the configured name is already on
/// something that is not a note file: a folder, or a link to one. Minting
/// would dedupe around it and hand back a second file for the same day, one
/// more on every ask, so the name is reported as taken instead. Only the
/// configured name is asked about: the other one is somewhere to read the day
/// from, never somewhere to write, so a folder wearing it blocks nothing.
pub fn find_todays_file(
    notes_root: &Path,
    now: DateTime<Utc>,
    configured: FileExtension,
) -> StorageResult<TodaysFileAt> {
    let names = writ_core::startup::dated_note_names(now, configured);
    for name in &names {
        let path = notes_root.join(name);
        if path.is_file() {
            return Ok(TodaysFileAt::Existing(path));
        }
    }
    let [minting, _] = names;
    let path = notes_root.join(&minting);
    if path.symlink_metadata().is_ok() {
        return Err(StorageError::NoteNameTaken {
            name: minting,
            folder: notes_root.to_path_buf(),
        });
    }
    Ok(TodaysFileAt::Missing(path))
}
