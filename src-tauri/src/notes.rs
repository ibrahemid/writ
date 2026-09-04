//! Minting the file a note has to have.
//!
//! The policy half — what a title becomes, how a collision dedupes — is
//! [`writ_core::notes`]. This is the half that touches the disk: it lists the
//! notes folder to learn which names are taken. Two callers share it, and
//! they have to agree or the same note would be named one way on its first
//! keystroke and another way after a crash: the first save of a new note
//! (`commands::buffer`) and the startup pass that restores a note the last
//! session never wrote (`state`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

/// Picks the path a note with `title` takes inside `notes_root`.
///
/// Dated when the title names nothing, sanitised for all three platforms, and
/// deduped Finder-style against what the folder already holds. The file is not
/// created: the save that follows writes it, and creating it here would leave
/// an empty file behind whenever that save fails.
///
/// A folder that cannot be listed yields no taken names rather than an error.
/// The dedupe would only be less exact, and refusing to name a note because
/// its folder could not be listed would lose the text the caller is holding.
pub fn mint_note_path(notes_root: &Path, title: &str, now: DateTime<Utc>) -> PathBuf {
    let stem = writ_core::notes::note_file_stem(title, now);
    let taken = taken_names(notes_root);
    notes_root.join(writ_core::notes::dedupe_file_name(&stem, "md", &taken))
}

/// The names `notes_root` already holds, lowercased the way the dedupe
/// compares them.
fn taken_names(notes_root: &Path) -> HashSet<String> {
    let Ok(entries) = std::fs::read_dir(notes_root) else {
        return HashSet::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().to_lowercase())
        .collect()
}

/// [`mint_note_path`] as the text a database row holds.
///
/// A path that would not survive the round trip back to a path is refused
/// rather than stored lossily: a note whose recorded path does not reopen is
/// a note nobody can find again.
pub fn mint_note_path_text(
    notes_root: &Path,
    title: &str,
    now: DateTime<Utc>,
) -> Result<String, String> {
    let path = mint_note_path(notes_root, title, now);
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| format!("the file name {} cannot be recorded", path.display()))
}

/// Gives a note that has no file the one the invariant requires, and records
/// it on the row (ADR-028 §2).
///
/// Returns the path the caller then writes to. The write is left to the
/// caller so exactly one code path writes a note's text.
pub fn attach_note_file(
    store: &writ_storage::buffer_store::BufferStore,
    notes_root: &Path,
    id: &str,
    title: &str,
    now: DateTime<Utc>,
) -> Result<String, String> {
    let path = mint_note_path_text(notes_root, title, now)?;
    store
        .attach_source_path(id, &path)
        .map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn day() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-28T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn a_title_nobody_typed_becomes_the_date() {
        let root = TempDir::new().unwrap();
        let path = mint_note_path(root.path(), "writ-1756000000000", day());
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.ends_with(".md"), "{name}");
        assert_eq!(name.len(), "2026-08-28.md".len(), "{name}");
    }

    #[test]
    fn a_name_already_in_the_folder_dedupes() {
        let root = TempDir::new().unwrap();
        std::fs::write(root.path().join("Notes.md"), "first").unwrap();
        let path = mint_note_path(root.path(), "Notes", day());
        assert_eq!(path, root.path().join("Notes 2.md"));
    }

    #[test]
    fn nothing_is_created_by_choosing_a_name() {
        let root = TempDir::new().unwrap();
        let path = mint_note_path(root.path(), "Notes", day());
        assert!(!path.exists());
    }
}
