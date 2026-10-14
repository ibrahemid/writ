use std::io;
use std::path::{Path, PathBuf};

use writ_core::workspace::{is_ignored_name, is_notes_ignored_name, DirListing, WorkspaceEntry};

use crate::errors::{StorageError, StorageResult};
use crate::paths::strip_verbatim_prefix;

/// Lists the immediate children of `dir` that are inside `workspace_root`.
///
/// Both paths are canonicalized before the containment check; symlinks that
/// resolve outside `workspace_root` are rejected. Entries whose name matches
/// the default ignore set, or one of the sync and editor leftover patterns
/// beside it, are excluded ([`is_ignored_name`]); a conflict copy is listed
/// with `conflict_copy` set rather than excluded. The entries are sorted:
/// directories first, then files, each group ordered case-insensitively.
///
/// A folder that cannot be read is an error. An entry inside it that cannot be
/// read is left out and counted in [`DirListing::unreadable`], so one bad
/// entry never hides the rest of the folder.
///
/// An entry's path is the canonical one without the Windows verbatim prefix
/// ([`strip_verbatim_prefix`]), the spelling
/// [`crate::notes_index::index_key`] keys the index by: the tree and the index
/// name the same note.
pub fn list_dir(workspace_root: &Path, dir: &Path) -> StorageResult<DirListing> {
    list_entries(workspace_root, dir, is_ignored_name)
}

/// [`list_dir`] for `dir`, a folder inside the notes folder `notes_root`.
///
/// The boundary, the symlink rule, the order and the spelling of each path are
/// [`list_dir`]'s. What it leaves out is what the notes walk skips
/// ([`is_notes_ignored_name`]) rather than what a code checkout hides, so a
/// note under `vendor/` or `build/` is in the tree as it is in search and
/// backlinks (ADR-028 section 7).
pub fn list_notes_dir(notes_root: &Path, dir: &Path) -> StorageResult<DirListing> {
    list_entries(notes_root, dir, is_notes_ignored_name)
}

/// What a directory read says an entry is, before a link is followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildKind {
    Dir,
    File,
    Symlink,
}

/// One child of a listed folder, as the directory read handed it over.
struct Child {
    name: String,
    path: PathBuf,
    kind: io::Result<ChildKind>,
}

/// The listing [`list_dir`] and [`list_notes_dir`] share, leaving out every
/// entry whose name `hides` answers for.
fn list_entries(
    workspace_root: &Path,
    dir: &Path,
    hides: impl Fn(&str) -> bool,
) -> StorageResult<DirListing> {
    let canonical_root = workspace_root.canonicalize().map_err(|e| {
        StorageError::Io(std::io::Error::new(
            e.kind(),
            format!(
                "workspace root not accessible: {}",
                workspace_root.display()
            ),
        ))
    })?;

    let canonical_dir = dir.canonicalize().map_err(|e| {
        StorageError::Io(std::io::Error::new(
            e.kind(),
            format!("directory not accessible: {}", dir.display()),
        ))
    })?;

    if !canonical_dir.starts_with(&canonical_root) {
        return Err(StorageError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "directory '{}' is outside workspace root '{}'",
                canonical_dir.display(),
                canonical_root.display()
            ),
        )));
    }

    let children = std::fs::read_dir(&canonical_dir)?.map(|entry| {
        entry.map(|entry| Child {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: entry.path(),
            kind: entry.file_type().map(|file_type| {
                if file_type.is_symlink() {
                    ChildKind::Symlink
                } else if file_type.is_dir() {
                    ChildKind::Dir
                } else {
                    ChildKind::File
                }
            }),
        })
    });
    Ok(collect_listing(&canonical_root, children, hides))
}

/// Describes each child the read handed over, leaving out the hidden ones and
/// links that lead out of `canonical_root`, and counting the ones it could not
/// read.
fn collect_listing(
    canonical_root: &Path,
    children: impl Iterator<Item = io::Result<Child>>,
    hides: impl Fn(&str) -> bool,
) -> DirListing {
    let mut entries = Vec::new();
    let mut unreadable: u32 = 0;

    for child in children {
        let Ok(child) = child else {
            unreadable = unreadable.saturating_add(1);
            continue;
        };
        if hides(&child.name) {
            continue;
        }
        let is_dir = match child.kind {
            Ok(ChildKind::Dir) => true,
            Ok(ChildKind::File) => false,
            Ok(ChildKind::Symlink) => {
                let resolved = child
                    .path
                    .canonicalize()
                    .unwrap_or_else(|_| child.path.clone());
                if !resolved.starts_with(canonical_root) {
                    continue;
                }
                resolved.is_dir()
            }
            Err(_) => {
                unreadable = unreadable.saturating_add(1);
                continue;
            }
        };
        entries.push(WorkspaceEntry::new(
            child.name,
            strip_verbatim_prefix(child.path)
                .to_string_lossy()
                .into_owned(),
            is_dir,
        ));
    }

    DirListing::new(entries, unreadable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn lists_files_and_dirs() {
        let root = setup();
        fs::write(root.path().join("main.rs"), "fn main() {}").unwrap();
        fs::create_dir(root.path().join("src")).unwrap();

        let entries = list_dir(root.path(), root.path()).unwrap().entries;
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"main.rs"));
        assert!(names.contains(&"src"));
    }

    #[test]
    fn dirs_come_before_files() {
        let root = setup();
        fs::write(root.path().join("aaa.rs"), "").unwrap();
        fs::create_dir(root.path().join("zzz_dir")).unwrap();

        let entries = list_dir(root.path(), root.path()).unwrap().entries;
        assert!(entries[0].is_dir, "first entry should be a dir");
    }

    #[test]
    fn ignores_default_ignored_names() {
        let root = setup();
        fs::create_dir(root.path().join("node_modules")).unwrap();
        fs::create_dir(root.path().join(".git")).unwrap();
        fs::write(root.path().join("main.rs"), "").unwrap();

        let entries = list_dir(root.path(), root.path()).unwrap().entries;
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(!names.contains(&"node_modules"));
        assert!(!names.contains(&".git"));
        assert!(names.contains(&"main.rs"));
    }

    #[test]
    fn a_notes_folder_lists_the_notes_and_the_copy_and_nothing_else() {
        // The fixture a synced notes folder actually holds: the note, the temp
        // file the sync client is writing, the placeholder standing in for a
        // file that is not downloaded, and the copy the client kept when two
        // devices wrote at once.
        let root = setup();
        fs::write(root.path().join("note.md"), "text").unwrap();
        fs::write(root.path().join(".syncthing.note.md.tmp"), "").unwrap();
        fs::write(root.path().join(".note.md.icloud"), "").unwrap();
        fs::write(
            root.path()
                .join("note.sync-conflict-20260822-120000-ABCD.md"),
            "text",
        )
        .unwrap();

        let entries = list_dir(root.path(), root.path()).unwrap().entries;
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["note.md", "note.sync-conflict-20260822-120000-ABCD.md"]
        );

        let copy = entries.last().unwrap();
        assert_eq!(
            copy.conflict_copy,
            Some(writ_core::workspace::ConflictCopyKind::SyncClient),
            "a copy is listed with the flag on it, never hidden"
        );
        assert_eq!(entries[0].conflict_copy, None);
    }

    #[test]
    fn the_notes_folder_lists_what_the_walk_reads_and_a_code_workspace_does_not() {
        let root = setup();
        fs::create_dir(root.path().join("vendor")).unwrap();
        fs::write(root.path().join("vendor").join("Acme.md"), "supplier").unwrap();
        fs::create_dir(root.path().join("build")).unwrap();
        fs::create_dir(root.path().join("node_modules")).unwrap();
        fs::create_dir(root.path().join(".obsidian")).unwrap();
        fs::create_dir(root.path().join(".git")).unwrap();
        fs::write(root.path().join(".DS_Store"), "").unwrap();
        fs::write(root.path().join(".writ-tmp-a1b2"), "").unwrap();
        fs::write(root.path().join("Home.md"), "see [[Acme]]").unwrap();

        let names = |entries: Vec<WorkspaceEntry>| -> Vec<String> {
            entries.into_iter().map(|e| e.name).collect()
        };
        assert_eq!(
            names(list_notes_dir(root.path(), root.path()).unwrap().entries),
            ["build", "node_modules", "vendor", "Home.md"]
        );
        assert_eq!(
            names(
                list_notes_dir(root.path(), &root.path().join("vendor"))
                    .unwrap()
                    .entries
            ),
            ["Acme.md"]
        );
        assert_eq!(
            names(list_dir(root.path(), root.path()).unwrap().entries),
            ["Home.md"]
        );
    }

    #[test]
    fn the_notes_folder_listing_keeps_its_boundary() {
        let root = setup();
        let notes = root.path().join("notes");
        fs::create_dir(&notes).unwrap();

        assert!(list_notes_dir(&notes, root.path()).is_err());
        assert!(list_notes_dir(&notes, &notes.join("..").join("..")).is_err());
    }

    #[test]
    fn rejects_path_traversal() {
        let root = setup();
        let escaped = root.path().join("..").join("etc");

        let result = list_dir(root.path(), &escaped);
        assert!(result.is_err(), "traversal must be rejected");
    }

    #[test]
    fn rejects_nonexistent_root() {
        let result = list_dir(
            Path::new("/nonexistent/xyz/abc123"),
            Path::new("/nonexistent/xyz/abc123"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn rejects_nonexistent_dir() {
        let root = setup();
        let missing = root.path().join("missing_subdir");

        let result = list_dir(root.path(), &missing);
        assert!(result.is_err());
    }

    #[test]
    fn lists_subdirectory_within_root() {
        let root = setup();
        let sub = root.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("lib.rs"), "").unwrap();

        let entries = list_dir(root.path(), &sub).unwrap().entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "lib.rs");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escaping_root() {
        use std::os::unix::fs::symlink;

        let root = setup();
        let tmp2 = setup();

        let link_path = root.path().join("escape_link");
        symlink(tmp2.path(), &link_path).unwrap();

        let result = list_dir(root.path(), &link_path);
        assert!(result.is_err(), "symlink escape must be rejected");
    }

    fn child(name: &str, kind: io::Result<ChildKind>) -> io::Result<Child> {
        Ok(Child {
            name: name.to_string(),
            path: PathBuf::from("/root").join(name),
            kind,
        })
    }

    #[test]
    fn an_entry_the_read_fails_on_is_counted_and_the_rest_are_listed() {
        let children = vec![
            child("a.md", Ok(ChildKind::File)),
            Err(io::Error::other("entry vanished")),
            child("docs", Ok(ChildKind::Dir)),
            child(
                "locked.md",
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "no stat")),
            ),
            child("b.md", Ok(ChildKind::File)),
        ];

        let listing = collect_listing(Path::new("/root"), children.into_iter(), |_| false);

        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["docs", "a.md", "b.md"]);
        assert_eq!(listing.unreadable, 2);
    }

    #[test]
    fn a_hidden_entry_is_left_out_without_being_counted() {
        let children = vec![
            child("node_modules", Ok(ChildKind::Dir)),
            child("note.md", Ok(ChildKind::File)),
        ];

        let listing = collect_listing(Path::new("/root"), children.into_iter(), is_ignored_name);

        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["note.md"]);
        assert_eq!(listing.unreadable, 0);
    }
}
