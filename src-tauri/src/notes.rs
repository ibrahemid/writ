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
use writ_core::config::FileExtension;

/// Picks the path a note with `title` takes inside `notes_root`, in
/// `extension`.
///
/// Dated when the title names nothing, sanitised for all three platforms, and
/// deduped by counter against what the folder already holds. `extension` is
/// the configured one (ADR-041 §2), so a file that reaches its name here is
/// the format every other mint makes. The file is not created: the save that
/// follows writes it, and creating it here would leave an empty file behind
/// whenever that save fails.
///
/// A folder that cannot be listed yields no taken names rather than an error.
/// The dedupe would only be less exact, and refusing to name a note because
/// its folder could not be listed would lose the text the caller is holding.
pub fn mint_note_path(
    notes_root: &Path,
    title: &str,
    now: DateTime<Utc>,
    extension: FileExtension,
) -> PathBuf {
    let stem = writ_core::notes::note_file_stem(title, now);
    let taken = taken_names(notes_root);
    notes_root.join(writ_core::notes::dedupe_file_name(
        &stem,
        extension.as_str(),
        &taken,
    ))
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
    extension: FileExtension,
) -> Result<String, String> {
    let path = mint_note_path(notes_root, title, now, extension);
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
    extension: FileExtension,
) -> Result<String, String> {
    let path = mint_note_path_text(notes_root, title, now, extension)?;
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
        let path = mint_note_path(root.path(), "writ-1756000000000", day(), FileExtension::Txt);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(name, "2026-08-28.txt", "{name}");
    }

    #[test]
    fn the_configured_format_names_the_file() {
        let root = TempDir::new().unwrap();
        assert_eq!(
            mint_note_path(root.path(), "Notes", day(), FileExtension::Txt),
            root.path().join("Notes.txt")
        );
        assert_eq!(
            mint_note_path(root.path(), "Notes", day(), FileExtension::Md),
            root.path().join("Notes.md")
        );
    }

    #[test]
    fn a_name_already_in_the_folder_dedupes() {
        let root = TempDir::new().unwrap();
        std::fs::write(root.path().join("Notes.txt"), "first").unwrap();
        let path = mint_note_path(root.path(), "Notes", day(), FileExtension::Txt);
        assert_eq!(path, root.path().join("Notes-2.txt"));
    }

    #[test]
    fn nothing_is_created_by_choosing_a_name() {
        let root = TempDir::new().unwrap();
        let path = mint_note_path(root.path(), "Notes", day(), FileExtension::Txt);
        assert!(!path.exists());
    }
}
