//! Moving the notes folder, and emptying the archive into it.
//!
//! Mechanism only, the way [`crate::notes_migration`] is: every naming and
//! dedupe decision comes from [`writ_core::notes`], and the policy around a
//! move — which folder was picked, what the config now says — belongs to the
//! caller.
//!
//! Three rules make a move safe to run against a folder holding somebody's
//! notes. Nothing moves at all when the destination already holds a name the
//! notes folder holds, so no file is ever written over. A move within one
//! volume is a rename per entry, and one that fails part way puts back
//! everything it had already moved. A move to another volume copies
//! everything into a staging folder at the destination, syncs it, renames it
//! into place, and only then removes the originals: until the last rename
//! the notes are whole where they were, and after it they are whole where
//! they went.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use tracing::warn;
use writ_core::notes;

use crate::buffer_store::BufferStore;
use crate::database::queries;
use crate::errors::{StorageError, StorageResult};
use crate::notes_migration;
use crate::paths::file_name_only;

/// What a move of the notes folder did.
///
/// `collided` is non-empty only when nothing moved: the destination already
/// held those names, and the user is told which ones rather than having a note
/// written over.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MoveOutcome {
    /// Entries moved out of the old folder.
    pub moved: usize,
    /// Names the destination already held, sorted.
    pub collided: Vec<String>,
    /// Entries copied to another volume whose originals could not all be
    /// removed, sorted. The copy at the destination is whole; what is left in
    /// the old folder is a second copy of it.
    pub left_behind: Vec<String>,
}

/// How an entry is moved, so the copy-across path can be exercised.
///
/// A seam of the same shape as `commands::notes::RecordRename`: the failure it
/// answers is a rename the filesystem will not make, which on one volume
/// nothing can provoke.
pub type Rename<'a> = &'a dyn Fn(&Path, &Path) -> std::io::Result<()>;

/// The prefix of the folder a move to another volume copies into before
/// anything takes its final name. A dot name, so no walk, watcher or listing
/// of the notes folder treats it as a note.
const STAGING_PREFIX: &str = ".writ-move-";

/// Room left over on the destination beyond the bytes copied, for the
/// directory entries and blocks a filesystem spends on them.
const SPACE_MARGIN: u64 = 1024 * 1024;

/// Moves every entry of `from` into `to`.
///
/// `to` is created when it does not exist. An entry is moved with
/// [`std::fs::rename`], which is atomic within one volume. A rename refused
/// because the two folders are on different volumes moves the rest by copy
/// ([`copy_across`]); a rename refused for any other reason puts back what
/// had moved and fails.
///
/// Dotfiles are left where they are, the same filter
/// [`move_archive_into_notes`] applies. `.DS_Store`, `.obsidian` and the rest
/// belong to whatever wrote them, not to Writ, and a `.DS_Store` at both ends
/// would otherwise refuse a move over a file nobody would miss.
///
/// # Errors
///
/// [`StorageError::Io`] when the destination cannot be created or read, or
/// when an entry can be neither renamed nor copied, in which case every entry
/// already moved is put back first; [`StorageError::FolderNotDownloaded`] and
/// [`StorageError::NotEnoughSpace`] when a copy to another volume is refused
/// before it starts.
pub fn move_notes_folder(from: &Path, to: &Path) -> StorageResult<MoveOutcome> {
    move_notes_folder_renaming(from, to, &|from, to| std::fs::rename(from, to))
}

/// [`move_notes_folder`] with the rename handed in.
pub fn move_notes_folder_renaming(
    from: &Path,
    to: &Path,
    rename: Rename<'_>,
) -> StorageResult<MoveOutcome> {
    move_with(from, to, rename, &Probes::FILESYSTEM)
}

/// What a copy to another volume asks before it writes anything, handed in so
/// a placeholder and a full disk can be exercised where neither exists.
struct Probes<'a> {
    /// Whether the entry at a path has no bytes on this machine.
    is_dataless: &'a dyn Fn(&Path) -> bool,
    /// The bytes the volume holding a folder has free, when it will say.
    available: &'a dyn Fn(&Path) -> Option<u64>,
}

impl Probes<'static> {
    const FILESYSTEM: Self = Self {
        is_dataless: &crate::notes_index::is_dataless,
        available: &available_bytes,
    };
}

fn move_with(
    from: &Path,
    to: &Path,
    rename: Rename<'_>,
    probes: &Probes<'_>,
) -> StorageResult<MoveOutcome> {
    std::fs::create_dir_all(to)?;

    let entries = entry_names(from)?;
    let taken = lowercased_names(to);
    let mut collided: Vec<String> = entries
        .iter()
        .filter(|name| taken.contains(&name.to_lowercase()))
        .cloned()
        .collect();
    if !collided.is_empty() {
        collided.sort();
        return Ok(MoveOutcome {
            collided,
            ..MoveOutcome::default()
        });
    }

    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (index, name) in entries.iter().enumerate() {
        let source = from.join(name);
        let destination = to.join(name);
        match rename(&source, &destination) {
            Ok(()) => done.push((source, destination)),
            Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
                let rest = &entries[index..];
                return match copy_across(from, to, rest, probes) {
                    Ok(left_behind) => Ok(MoveOutcome {
                        moved: done.len() + rest.len(),
                        collided: Vec::new(),
                        left_behind,
                    }),
                    Err(error) => {
                        put_back(rename, &done);
                        Err(error)
                    }
                };
            }
            Err(error) => {
                put_back(rename, &done);
                return Err(error.into());
            }
        }
    }

    Ok(MoveOutcome {
        moved: done.len(),
        ..MoveOutcome::default()
    })
}

/// Puts every entry a failed move had already renamed back where it was.
///
/// Best effort: an entry that will not go back is logged and the rest are
/// still tried, because stopping here would leave more of them stranded than
/// carrying on does.
fn put_back(rename: Rename<'_>, done: &[(PathBuf, PathBuf)]) {
    for (was, is) in done.iter().rev() {
        if let Err(error) = rename(is, was) {
            warn!(
                path = %file_name_only(&is.to_string_lossy()),
                %error,
                "an entry could not be put back after a failed move"
            );
        }
    }
}

/// Copies the entries `names` of `from` into `to` on another volume, then
/// removes the originals, and answers the names whose originals could not
/// all be removed.
///
/// In order: every file is checked to be on this machine and the destination
/// to have room, before anything is written; everything is copied into a
/// staging folder inside `to`, each file and folder synced; each staged entry
/// is renamed to its name, refusing a name something took in between; `to`
/// is synced; and only then are the originals removed. A failure before the
/// last rename leaves `from` as it was and clears the staging folder. A
/// removal that fails afterwards is not a failed move: the notes are whole at
/// the destination, and what is left in `from` is reported.
///
/// Symlinks are copied as links, never followed.
fn copy_across(
    from: &Path,
    to: &Path,
    names: &[String],
    probes: &Probes<'_>,
) -> StorageResult<Vec<String>> {
    let mut needed: u64 = 0;
    let mut dataless: Vec<String> = Vec::new();
    for name in names {
        survey(&from.join(name), probes, &mut needed, &mut dataless)?;
    }
    if let Some(first) = dataless.first() {
        return Err(StorageError::FolderNotDownloaded {
            count: dataless.len(),
            first: first.clone(),
        });
    }
    let needed = needed
        .saturating_add(needed / 20)
        .saturating_add(SPACE_MARGIN);
    if let Some(available) = (probes.available)(to) {
        if needed > available {
            return Err(StorageError::NotEnoughSpace { needed, available });
        }
    }

    let staging = tempfile::Builder::new()
        .prefix(STAGING_PREFIX)
        .tempdir_in(to)?;
    for name in names {
        copy_entry(&from.join(name), &staging.path().join(name))?;
    }
    sync_dir(staging.path())?;

    let mut placed: Vec<&String> = Vec::new();
    for name in names {
        let staged = staging.path().join(name);
        if let Err(error) = crate::note_ops::rename_no_replace(&staged, &to.join(name)) {
            for name in placed.iter().rev() {
                if let Err(error) =
                    crate::note_ops::rename_no_replace(&to.join(name), &staging.path().join(name))
                {
                    warn!(
                        path = %file_name_only(name),
                        %error,
                        "a copied entry could not be taken back after a failed move"
                    );
                }
            }
            return Err(error.into());
        }
        placed.push(name);
    }
    sync_dir(to)?;
    drop(staging);

    let mut left_behind = Vec::new();
    for name in names {
        if let Err(error) = remove_tree(&from.join(name)) {
            warn!(
                path = %file_name_only(name),
                %error,
                "a moved entry's original could not be removed"
            );
            left_behind.push(name.clone());
        }
    }
    left_behind.sort();
    Ok(left_behind)
}

/// Adds up what copying `path` would write and names what under it has no
/// bytes on this machine, without following a symlink or reading a file.
fn survey(
    path: &Path,
    probes: &Probes<'_>,
    bytes: &mut u64,
    dataless: &mut Vec<String>,
) -> StorageResult<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if (probes.is_dataless)(path) {
        dataless.push(file_name_only(&path.to_string_lossy()));
        return Ok(());
    }
    if metadata.file_type().is_dir() {
        for entry in std::fs::read_dir(path)? {
            survey(&entry?.path(), probes, bytes, dataless)?;
        }
    } else if metadata.file_type().is_file() {
        *bytes = bytes.saturating_add(metadata.len());
    }
    Ok(())
}

/// Copies `from` to `to`, a folder with everything in it, syncing each file
/// and folder it writes. A symlink is copied as the link it is.
fn copy_entry(from: &Path, to: &Path) -> std::io::Result<()> {
    let kind = std::fs::symlink_metadata(from)?.file_type();
    if kind.is_symlink() {
        return copy_link(from, to);
    }
    if kind.is_dir() {
        std::fs::create_dir(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_entry(&entry.path(), &to.join(entry.file_name()))?;
        }
        return sync_dir(to);
    }
    std::fs::copy(from, to)?;
    sync_file(to)
}

#[cfg(unix)]
fn copy_link(from: &Path, to: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(std::fs::read_link(from)?, to)
}

#[cfg(windows)]
fn copy_link(from: &Path, to: &Path) -> std::io::Result<()> {
    let target = std::fs::read_link(from)?;
    if std::fs::metadata(from).is_ok_and(|meta| meta.is_dir()) {
        std::os::windows::fs::symlink_dir(target, to)
    } else {
        std::os::windows::fs::symlink_file(target, to)
    }
}

#[cfg(not(any(unix, windows)))]
fn copy_link(_from: &Path, _to: &Path) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

/// Flushes a copied file to the disk.
#[cfg(unix)]
fn sync_file(path: &Path) -> std::io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

/// Flushes a copied file to the disk. Windows flushes only a handle open for
/// writing, which a read-only file will not give; its copy is left to the
/// system's own flush.
#[cfg(not(unix))]
fn sync_file(path: &Path) -> std::io::Result<()> {
    match std::fs::OpenOptions::new().write(true).open(path) {
        Ok(file) => file.sync_all(),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Ok(()),
        Err(error) => Err(error),
    }
}

/// Flushes a folder's entries to the disk, so a file synced inside it is
/// also found under its name after a crash.
#[cfg(unix)]
fn sync_dir(path: &Path) -> std::io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

/// Windows records a folder's entries with the files themselves.
#[cfg(not(unix))]
fn sync_dir(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// The bytes the volume holding `dir` has free for this user, or `None` when
/// the platform will not say.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[allow(unsafe_code, clippy::unnecessary_cast)]
fn available_bytes(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: an all-zero `statvfs` is a valid value of the plain C struct the
    // call fills in.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is NUL-terminated and outlives the call, and `stat` is a
    // valid place for the answer.
    let status = unsafe { libc::statvfs(path.as_ptr(), &mut stat) };
    (status == 0).then(|| (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64))
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn available_bytes(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut available: u64 = 0;
    // SAFETY: `wide` is NUL-terminated UTF-16 that outlives the call, and
    // `available` is a valid place for the answer.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut available), None, None) }.ok()?;
    Some(available)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn available_bytes(_dir: &Path) -> Option<u64> {
    None
}

/// Moves one archived file into the notes folder, copying it across when the
/// archive is on another volume.
///
/// The copy is synced beside its destination and renamed into place, so the
/// notes folder never holds part of it. An original that cannot be removed
/// afterwards is logged and the move stands: the file is whole where it went.
fn move_archived_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {}
        other => return other,
    }
    let folder = to.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the destination has no folder",
        )
    })?;
    let staging = tempfile::Builder::new()
        .prefix(STAGING_PREFIX)
        .tempdir_in(folder)?;
    let staged = staging.path().join("file");
    copy_entry(from, &staged)?;
    crate::note_ops::rename_no_replace(&staged, to)?;
    sync_dir(folder)?;
    drop(staging);
    if let Err(error) = remove_tree(from) {
        warn!(
            path = %file_name_only(&from.to_string_lossy()),
            %error,
            "an archived note's original could not be removed"
        );
    }
    Ok(())
}

/// Removes a file, or a folder and everything in it, without following a
/// symlink.
fn remove_tree(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(metadata) if metadata.file_type().is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
    }
}

/// Every entry directly inside `dir` that is Writ's to move, sorted so a move
/// is repeatable. Dotfiles are not.
fn entry_names(dir: &Path) -> StorageResult<Vec<String>> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    Ok(names)
}

/// The names `dir` already holds, lowercased the way the dedupe compares them.
fn lowercased_names(dir: &Path) -> HashSet<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return HashSet::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().to_lowercase())
        .collect()
}

/// One row whose file is now somewhere else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repointed {
    /// The note.
    pub id: String,
    /// Where its file was.
    pub from: String,
    /// Where its file is now.
    pub to: String,
}

/// Points every row under `from` at the same file under `to`.
///
/// Both the file a row names and the file the notes migration placed for it
/// are rewritten. A row left naming the old folder is a note nobody can open
/// again, and a migration record left naming it makes the next launch read the
/// row as unfinished work and run the whole pass over it.
///
/// Returns the rows whose file moved, so a caller holding a record of what
/// each file last held can move that record with them.
///
/// One transaction: a crash or an error part way leaves every row naming the
/// old folder, never some of each.
pub fn repoint_rows(store: &BufferStore, from: &Path, to: &Path) -> StorageResult<Vec<Repointed>> {
    let transaction = store.connection().unchecked_transaction()?;
    let mut moved = Vec::new();

    for (id, path) in queries::list_source_paths(&transaction)? {
        let Some(destination) = rebase(&path, from, to) else {
            continue;
        };
        queries::update_source_path(&transaction, &id, &destination)?;
        moved.push(Repointed {
            id,
            from: path,
            to: destination,
        });
    }

    for (id, path) in queries::list_migrated_paths(&transaction)? {
        let Some(destination) = rebase(&path, from, to) else {
            continue;
        };
        queries::set_migrated_path(&transaction, &id, &destination)?;
    }

    transaction.commit()?;
    Ok(moved)
}

/// `path` with its `from` prefix replaced by `to`, or `None` when `from` does
/// not contain it.
///
/// Component-wise, so a move out of `~/Writ` never claims a file in
/// `~/Writing`.
fn rebase(path: &str, from: &Path, to: &Path) -> Option<String> {
    let relative = Path::new(path).strip_prefix(from).ok()?;
    Some(to.join(relative).to_string_lossy().into_owned())
}

/// What emptying the archive into the notes folder did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArchiveMoveOutcome {
    /// Files moved into the notes folder.
    pub moved: usize,
    /// The names that were already taken, so the file arrived under a deduped
    /// one. Sorted, and reported under the name the user would look for.
    pub collided: Vec<String>,
}

/// Moves every file the notes migration archived into the notes folder.
///
/// The archive is where a history row's text waits until the user asks for it
/// (ADR-028 section 4 step 3), so this is that answer. A name the notes folder
/// already holds is deduped by counter rather than refused: the user asked
/// for these files to arrive, and forty of them stopping on one clash would
/// leave the archive half empty with nothing to do about it.
///
/// Each moved file's row is pointed at it, which is what turns an archived
/// note back into an ordinary one, and the stored report is re-counted so the
/// offer to move them does not come back on the next launch.
///
/// Refuses outright when the two folders are one, or when either holds the
/// other. Every name in the archive would then already be taken by the file
/// itself, so the dedupe above would rename each note onto a numbered copy of
/// itself and rewrite its row to match.
pub fn move_archive_into_notes(
    store: &BufferStore,
    archive: &Path,
    notes: &Path,
    now: DateTime<Utc>,
) -> StorageResult<ArchiveMoveOutcome> {
    refuse_overlap(archive, notes)?;

    let files = archived_files(archive);
    if files.is_empty() {
        return Ok(ArchiveMoveOutcome::default());
    }
    std::fs::create_dir_all(notes)?;

    let owners = owners_by_archived_path(store)?;
    let mut taken = lowercased_names(notes);
    let mut outcome = ArchiveMoveOutcome::default();
    let mut placed: Vec<(String, String)> = Vec::new();

    for file in files {
        let was = file_name(&file);
        let stem = notes::sanitize_title_or(&file_stem(&file), &notes::date_stem(now));
        let name = notes::dedupe_file_name(&stem, &extension(&file), &taken);
        taken.insert(name.to_lowercase());
        let destination = notes.join(&name);

        if let Err(error) = move_archived_file(&file, &destination) {
            warn!(
                path = %file_name_only(&file.to_string_lossy()),
                %error,
                "an archived note could not be moved"
            );
            continue;
        }
        if name != was {
            outcome.collided.push(was);
        }
        outcome.moved += 1;

        let text = path_text(&destination);
        placed.push((path_text(&file), text.clone()));

        let Some(id) = owners.get(&path_text(&file)) else {
            continue;
        };
        store.update_source_path(id, &text)?;
        queries::set_migrated_path(store.connection(), id, &text)?;
    }

    notes_migration::record_archive_moved(store, &placed)?;
    outcome.collided.sort();
    Ok(outcome)
}

/// Refuses when the archive and the notes folder are not two separate folders.
///
/// Both sides are resolved first, so the answer is about folders rather than
/// spellings and a linked folder cannot hide one inside the other. A side that
/// does not resolve does not exist, and an archive or a notes folder that is
/// not there holds nothing either way, so its own spelling is compared.
fn refuse_overlap(archive: &Path, notes: &Path) -> StorageResult<()> {
    let resolved = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let (from, to) = (resolved(archive), resolved(notes));
    if from.starts_with(&to) || to.starts_with(&from) {
        return Err(StorageError::ArchiveHoldsNotes {
            archive: from,
            notes: to,
        });
    }
    Ok(())
}

/// Every regular file directly inside the archive, sorted so a run is
/// repeatable.
///
/// Every file, not only the ones named `.md`: the migration writes `.txt` for
/// a note whose bytes are not text, and leaving those behind would empty the
/// folder of everything but the notes nobody could read.
fn archived_files(archive: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(archive) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| !file_name(path).starts_with('.'))
        .collect();
    files.sort();
    files
}

/// Maps the archived file each row was placed in to the row's id.
fn owners_by_archived_path(store: &BufferStore) -> StorageResult<HashMap<String, String>> {
    let mut owners = HashMap::new();
    for (id, path) in queries::list_migrated_paths(store.connection())? {
        owners.insert(path, id);
    }
    Ok(owners)
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|ext| ext.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prefix_is_replaced_component_by_component() {
        let from = Path::new("/home/u/Writ");
        let to = Path::new("/home/u/Notes");
        // Compared as a path: `join` spells the rebased half with the host's
        // separator, so on Windows the string carries a backslash.
        assert_eq!(
            rebase("/home/u/Writ/a/b.md", from, to).map(PathBuf::from),
            Some(PathBuf::from("/home/u/Notes/a/b.md"))
        );
        assert_eq!(rebase("/home/u/Writing/b.md", from, to), None);
        assert_eq!(rebase("/elsewhere/b.md", from, to), None);
    }

    #[test]
    fn a_move_into_a_folder_holding_the_same_name_moves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(from.join("Notes.md"), "mine").unwrap();
        std::fs::write(from.join("Other.md"), "mine").unwrap();
        std::fs::write(to.join("notes.md"), "theirs").unwrap();

        let outcome = move_notes_folder(&from, &to).unwrap();
        assert_eq!(outcome.moved, 0);
        assert_eq!(outcome.collided, vec!["Notes.md".to_string()]);
        assert!(from.join("Other.md").exists());
        assert_eq!(
            std::fs::read_to_string(to.join("notes.md")).unwrap(),
            "theirs"
        );
    }

    /// A rename the filesystem will not make, which is what a move across
    /// volumes looks like.
    fn refuse_rename(_: &Path, _: &Path) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::CrossesDevices))
    }

    /// A rename the filesystem refused for a reason a copy would not get
    /// around.
    fn deny_rename(_: &Path, _: &Path) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
    }

    #[test]
    fn a_rename_refused_for_another_reason_copies_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::write(from.join("Notes.md"), "one").unwrap();

        let error = move_notes_folder_renaming(&from, &to, &deny_rename).unwrap_err();

        assert!(
            matches!(error, crate::errors::StorageError::Io(_)),
            "{error}"
        );
        assert!(!to.join("Notes.md").exists(), "nothing was copied across");
        assert_eq!(
            std::fs::read_to_string(from.join("Notes.md")).unwrap(),
            "one"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_remove_that_fails_part_way_leaves_every_file_at_the_destination() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        std::fs::create_dir_all(from.join("projects")).unwrap();
        std::fs::write(from.join("Notes.md"), "one").unwrap();
        std::fs::write(from.join("projects").join("a.md"), "two").unwrap();
        std::fs::write(from.join("projects").join("b.md"), "three").unwrap();
        // A folder whose entries cannot be unlinked: the copy reads it, the
        // remove of the original fails inside it.
        let locked = from.join("projects");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();

        let outcome = move_notes_folder_renaming(&from, &to, &refuse_rename);

        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        let outcome = outcome.expect("every file reached the destination");
        assert_eq!(outcome.moved, 2);
        assert_eq!(outcome.left_behind, vec!["projects".to_string()]);
        assert!(
            !from.join("Notes.md").exists(),
            "a removable original was removed"
        );
        assert_eq!(std::fs::read_to_string(to.join("Notes.md")).unwrap(), "one");
        assert_eq!(
            std::fs::read_to_string(to.join("projects").join("a.md")).unwrap(),
            "two"
        );
        assert_eq!(
            std::fs::read_to_string(to.join("projects").join("b.md")).unwrap(),
            "three"
        );
    }

    #[test]
    fn dotfiles_stay_where_they_are_and_never_refuse_a_move() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(from.join(".DS_Store"), "theirs").unwrap();
        std::fs::write(to.join(".DS_Store"), "theirs too").unwrap();
        std::fs::write(from.join("Notes.md"), "mine").unwrap();

        let outcome = move_notes_folder(&from, &to).unwrap();

        assert!(outcome.collided.is_empty(), "{:?}", outcome.collided);
        assert_eq!(outcome.moved, 1);
        assert_eq!(
            std::fs::read_to_string(to.join("Notes.md")).unwrap(),
            "mine"
        );
        assert!(from.join(".DS_Store").exists(), "the dotfile stayed put");
        assert_eq!(
            std::fs::read_to_string(to.join(".DS_Store")).unwrap(),
            "theirs too"
        );
    }

    #[test]
    fn a_rename_that_cannot_be_made_copies_and_then_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        std::fs::create_dir_all(from.join("projects")).unwrap();
        std::fs::write(from.join("Notes.md"), "one").unwrap();
        std::fs::write(from.join("projects").join("Deep.md"), "two").unwrap();

        let outcome = move_notes_folder_renaming(&from, &to, &refuse_rename).unwrap();

        assert_eq!(outcome.moved, 2);
        assert_eq!(std::fs::read_to_string(to.join("Notes.md")).unwrap(), "one");
        assert_eq!(
            std::fs::read_to_string(to.join("projects").join("Deep.md")).unwrap(),
            "two"
        );
        assert_eq!(entry_names(&from).unwrap(), Vec::<String>::new());
    }

    #[cfg(unix)]
    #[test]
    fn a_copy_that_fails_part_way_leaves_neither_half_behind() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        std::fs::create_dir_all(from.join("projects")).unwrap();
        // Sorted before "projects", so it is moved before the copy fails.
        std::fs::write(from.join("Notes.md"), "one").unwrap();
        std::fs::write(from.join("projects").join("ok.md"), "two").unwrap();
        let unreadable = from.join("projects").join("locked.md");
        std::fs::write(&unreadable, "three").unwrap();
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();

        let error = move_notes_folder_renaming(&from, &to, &refuse_rename).unwrap_err();

        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            matches!(error, crate::errors::StorageError::Io(_)),
            "{error}"
        );
        assert!(
            !to.join("projects").exists(),
            "the part-copied folder was cleared"
        );
        assert!(
            !to.join("Notes.md").exists(),
            "the entry already copied never took its name"
        );
        assert!(
            std::fs::read_dir(&to).unwrap().next().is_none(),
            "the staging folder was cleared"
        );
        assert_eq!(
            std::fs::read_to_string(from.join("Notes.md")).unwrap(),
            "one"
        );
        assert_eq!(
            std::fs::read_to_string(from.join("projects").join("ok.md")).unwrap(),
            "two"
        );
    }

    /// A folder to move with a note at the top and one in a subfolder.
    fn two_notes(dir: &Path) -> (PathBuf, PathBuf) {
        let from = dir.join("from");
        let to = dir.join("to");
        std::fs::create_dir_all(from.join("projects")).unwrap();
        std::fs::write(from.join("Notes.md"), "one").unwrap();
        std::fs::write(from.join("projects").join("Deep.md"), "two").unwrap();
        (from, to)
    }

    fn untouched(from: &Path, to: &Path) {
        assert_eq!(
            std::fs::read_to_string(from.join("Notes.md")).unwrap(),
            "one"
        );
        assert_eq!(
            std::fs::read_to_string(from.join("projects").join("Deep.md")).unwrap(),
            "two"
        );
        assert!(
            std::fs::read_dir(to).unwrap().next().is_none(),
            "nothing was copied and no staging folder is left"
        );
    }

    #[test]
    fn a_folder_holding_a_placeholder_is_refused_before_anything_is_copied() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = two_notes(dir.path());
        let probes = Probes {
            is_dataless: &|path: &Path| path.ends_with("Deep.md"),
            available: &|_: &Path| None,
        };

        let error = move_with(&from, &to, &refuse_rename, &probes).unwrap_err();

        match error {
            StorageError::FolderNotDownloaded { count, first } => {
                assert_eq!(count, 1);
                assert_eq!(first, "Deep.md");
            }
            other => panic!("expected a refusal, got {other}"),
        }
        untouched(&from, &to);
    }

    #[test]
    fn a_destination_without_room_is_refused_before_anything_is_copied() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = two_notes(dir.path());
        let probes = Probes {
            is_dataless: &|_: &Path| false,
            available: &|_: &Path| Some(1),
        };

        let error = move_with(&from, &to, &refuse_rename, &probes).unwrap_err();

        assert!(
            matches!(error, StorageError::NotEnoughSpace { available: 1, .. }),
            "{error}"
        );
        untouched(&from, &to);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_copied_as_a_link_and_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("big.log"), "not a note").unwrap();
        std::os::unix::fs::symlink(&elsewhere, from.join("linked")).unwrap();

        let outcome = move_notes_folder_renaming(&from, &to, &refuse_rename).unwrap();

        assert_eq!(outcome.moved, 1);
        let moved = to.join("linked");
        assert!(std::fs::symlink_metadata(&moved)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read_link(&moved).unwrap(), elsewhere);
        assert!(
            elsewhere.join("big.log").exists(),
            "removing the original link left what it pointed at alone"
        );
    }

    #[test]
    fn a_copy_across_leaves_no_staging_folder_behind() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        std::fs::create_dir_all(from.join("projects")).unwrap();
        std::fs::write(from.join("Notes.md"), "one").unwrap();
        std::fs::write(from.join("projects").join("Deep.md"), "two").unwrap();

        move_notes_folder_renaming(&from, &to, &refuse_rename).unwrap();

        let mut names: Vec<String> = std::fs::read_dir(&to)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["Notes.md".to_string(), "projects".to_string()]);
    }

    fn store_with_rows(dir: &Path, paths: &[&str]) -> BufferStore {
        let conn = crate::database::connection::open_database(&dir.join("writ.db")).unwrap();
        crate::database::migrations::run_migrations(&conn).unwrap();
        let buffers = dir.join("buffers");
        std::fs::create_dir_all(&buffers).unwrap();
        let store = BufferStore::new(conn, buffers);
        let mut manager = writ_core::buffer::manager::BufferManager::new();
        for path in paths {
            let doc = manager.create_buffer(None).unwrap();
            store.insert(&doc).unwrap();
            store.attach_source_path(&doc.id, path).unwrap();
        }
        store
    }

    fn source_paths(store: &BufferStore) -> Vec<String> {
        let mut paths: Vec<String> = queries::list_source_paths(store.connection())
            .unwrap()
            .into_iter()
            .map(|(_, path)| path)
            .collect();
        paths.sort();
        paths
    }

    #[test]
    fn rows_are_repointed_all_together_or_not_at_all() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("Writ");
        let to = dir.path().join("Moved");
        let first = from.join("a.md").to_string_lossy().into_owned();
        let second = from.join("b.md").to_string_lossy().into_owned();
        let store = store_with_rows(dir.path(), &[&first, &second]);
        // The second row refuses its update, the way a disk error part way
        // through would.
        store
            .connection()
            .execute_batch(&format!(
                "CREATE TEMP TRIGGER refuse BEFORE UPDATE OF source_path ON buffers \
                 WHEN OLD.source_path = '{second}' BEGIN SELECT RAISE(ABORT, 'refused'); END;"
            ))
            .unwrap();

        assert!(repoint_rows(&store, &from, &to).is_err());
        assert_eq!(source_paths(&store), vec![first.clone(), second.clone()]);

        store
            .connection()
            .execute_batch("DROP TRIGGER refuse;")
            .unwrap();
        let moved = repoint_rows(&store, &from, &to).unwrap();
        assert_eq!(moved.len(), 2);
        assert!(source_paths(&store)
            .iter()
            .all(|path| Path::new(path).starts_with(&to)));
    }

    #[test]
    fn every_entry_including_a_subfolder_moves() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        std::fs::create_dir_all(from.join("projects")).unwrap();
        std::fs::write(from.join("Notes.md"), "one").unwrap();
        std::fs::write(from.join("projects").join("Deep.md"), "two").unwrap();

        let outcome = move_notes_folder(&from, &to).unwrap();
        assert_eq!(outcome.moved, 2);
        assert!(outcome.collided.is_empty());
        assert_eq!(std::fs::read_to_string(to.join("Notes.md")).unwrap(), "one");
        assert_eq!(
            std::fs::read_to_string(to.join("projects").join("Deep.md")).unwrap(),
            "two"
        );
        assert_eq!(entry_names(&from).unwrap(), Vec::<String>::new());
    }
}
