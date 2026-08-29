//! Moving the notes folder, and emptying the archive into it.
//!
//! Mechanism only, the way [`crate::notes_migration`] is: every naming and
//! dedupe decision comes from [`writ_core::notes`], and the policy around a
//! move — which folder was picked, what the config now says — belongs to the
//! caller.
//!
//! Two rules make a move safe to run against a folder holding somebody's
//! notes. Nothing moves at all when the destination already holds a name the
//! notes folder holds, so no file is ever written over. And a move that fails
//! part way puts back everything it had already moved and clears whatever the
//! failed entry left behind, so the notes are in one folder when it returns,
//! never spread across two.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use tracing::warn;
use writ_core::notes;

use crate::buffer_store::BufferStore;
use crate::database::queries;
use crate::errors::StorageResult;
use crate::notes_migration;

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
}

/// How an entry is moved, so the copy-across path can be exercised.
///
/// A seam of the same shape as `commands::notes::RecordRename`: the failure it
/// answers is a rename the filesystem will not make, which on one volume
/// nothing can provoke.
pub type Rename<'a> = &'a dyn Fn(&Path, &Path) -> std::io::Result<()>;

/// Moves every entry of `from` into `to`.
///
/// `to` is created when it does not exist. An entry is moved with
/// [`std::fs::rename`], which is atomic within one volume; a rename that
/// cannot be made — a different volume is the ordinary reason — falls back to
/// a copy followed by a delete of the original.
///
/// Dotfiles are left where they are, the same filter
/// [`move_archive_into_notes`] applies. `.DS_Store`, `.obsidian` and the rest
/// belong to whatever wrote them, not to Writ, and a `.DS_Store` at both ends
/// would otherwise refuse a move over a file nobody would miss.
///
/// # Errors
///
/// [`crate::errors::StorageError::Io`] when the destination cannot be created
/// or read, or when an entry can be neither renamed nor copied. In the last
/// case every entry already moved is put back first.
pub fn move_notes_folder(from: &Path, to: &Path) -> StorageResult<MoveOutcome> {
    move_notes_folder_renaming(from, to, &|from, to| std::fs::rename(from, to))
}

/// [`move_notes_folder`] with the rename handed in.
pub fn move_notes_folder_renaming(
    from: &Path,
    to: &Path,
    rename: Rename<'_>,
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
        return Ok(MoveOutcome { moved: 0, collided });
    }

    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    for name in &entries {
        let source = from.join(name);
        let destination = to.join(name);
        if let Err(error) = move_entry(rename, &source, &destination) {
            put_back(rename, &done);
            return Err(error.into());
        }
        done.push((source, destination));
    }

    Ok(MoveOutcome {
        moved: done.len(),
        collided: Vec::new(),
    })
}

/// Puts every entry a failed move had already moved back where it was.
///
/// Best effort: an entry that will not go back is logged and the rest are
/// still tried, because stopping here would leave more of them stranded than
/// carrying on does.
fn put_back(rename: Rename<'_>, done: &[(PathBuf, PathBuf)]) {
    for (was, is) in done.iter().rev() {
        if let Err(error) = move_entry(rename, is, was) {
            warn!(path = %is.display(), %error, "an entry could not be put back after a failed move");
        }
    }
}

/// Renames one entry, copying it across when a rename cannot be made.
///
/// A copy that fails part way is cleared before the error is returned. Half a
/// note at the destination is worse than none: the original is still where it
/// was, and a truncated twin of it would read as the note.
fn move_entry(rename: Rename<'_>, from: &Path, to: &Path) -> std::io::Result<()> {
    match rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            if let Err(error) = copy_tree(from, to) {
                if let Err(cleanup) = remove_tree(to) {
                    warn!(path = %to.display(), %cleanup, "a part-copied entry could not be cleared");
                }
                return Err(error);
            }
            remove_tree(from)
        }
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        return Ok(());
    }
    std::fs::copy(from, to).map(|_| ())
}

fn remove_tree(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
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
pub fn repoint_rows(store: &BufferStore, from: &Path, to: &Path) -> StorageResult<Vec<Repointed>> {
    let conn = store.connection();
    let mut moved = Vec::new();

    for (id, path) in queries::list_source_paths(conn)? {
        let Some(destination) = rebase(&path, from, to) else {
            continue;
        };
        queries::update_source_path(conn, &id, &destination)?;
        moved.push(Repointed {
            id,
            from: path,
            to: destination,
        });
    }

    for (id, path) in queries::list_migrated_paths(conn)? {
        let Some(destination) = rebase(&path, from, to) else {
            continue;
        };
        queries::set_migrated_path(conn, &id, &destination)?;
    }

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
/// already holds is deduped Finder-style rather than refused: the user asked
/// for these files to arrive, and forty of them stopping on one clash would
/// leave the archive half empty with nothing to do about it.
///
/// Each moved file's row is pointed at it, which is what turns an archived
/// note back into an ordinary one, and the stored report is re-counted so the
/// offer to move them does not come back on the next launch.
pub fn move_archive_into_notes(
    store: &BufferStore,
    archive: &Path,
    notes: &Path,
    now: DateTime<Utc>,
) -> StorageResult<ArchiveMoveOutcome> {
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

        if let Err(error) = move_entry(&|from, to| std::fs::rename(from, to), &file, &destination) {
            warn!(path = %file.display(), %error, "an archived note could not be moved");
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
        .unwrap_or_else(|| crate::note_ops::NOTE_EXTENSION.to_string())
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
        assert_eq!(
            rebase("/home/u/Writ/a/b.md", from, to).as_deref(),
            Some("/home/u/Notes/a/b.md")
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
        Err(std::io::Error::from_raw_os_error(18))
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
            "the entry already moved was put back"
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
