//! Whether a path a caller handed in names a file inside the notes folder.
//!
//! The rule is one rule for every surface that takes a path it did not choose:
//! the webview's `show_notes_file_in_file_manager`, the write gate, and an MCP
//! client's tool argument (ADR-031 rule 3.7). Resolution happens first and the
//! comparison happens on the resolved path, so neither a `..` nor a symlink
//! above the file carries the answer out of the folder.

use std::path::{Path, PathBuf};

/// Resolves `path` against the filesystem as far as it exists, then appends the
/// components that do not exist yet.
///
/// Two callers need the answer for a path that does not exist yet: the write
/// gate's containment check, for a note being minted, and the data-folder
/// guard, which has to know where `WRIT_DATA_DIR` will land before anything
/// creates it.
///
/// Walking up to the deepest existing ancestor is what makes the answer honest
/// for a file that is about to be created: every symlink and every `..` above
/// the new name is resolved by `canonicalize`, and only names the filesystem
/// has never seen are appended literally.
///
/// Returns `None` for a relative path, for a path whose unresolved tail is
/// `..` or empty (`Path::file_name` yields nothing for either, so such a tail
/// can never be appended), and for any resolution error other than a missing
/// file.
pub fn resolve_for_containment(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }

    let mut unresolved: Vec<std::ffi::OsString> = Vec::new();
    let mut cursor = path.to_path_buf();
    loop {
        match std::fs::canonicalize(&cursor) {
            Ok(base) => {
                let mut resolved = base;
                for name in unresolved.iter().rev() {
                    resolved.push(name);
                }
                return Some(resolved);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                unresolved.push(cursor.file_name()?.to_os_string());
                cursor = cursor.parent()?.to_path_buf();
            }
            Err(_) => return None,
        }
    }
}

/// Whether `resolved` is `root` or something under it.
///
/// `resolved` is expected to have been through [`resolve_for_containment`]
/// already. A `..` left in it means it did not, so it is refused rather than
/// compared: a prefix match against a path that still walks upwards answers
/// for a folder the caller never named.
pub fn is_inside(root: &Path, resolved: &Path) -> bool {
    if resolved
        .components()
        .any(|component| component == std::path::Component::ParentDir)
    {
        return false;
    }
    resolved.starts_with(root)
}

/// The resolved form of `path` when the folder at `root` holds it, and `None`
/// when it does not.
///
/// `root` is compared as given, so a caller holding a root that has not been
/// canonicalised passes one through [`resolve_for_containment`] first.
/// `/var/folders/...` on macOS is a link to `/private/var/folders/...`, and a
/// resolved file under a root spelled the first way matches neither.
pub fn resolve_inside(root: &Path, path: &Path) -> Option<PathBuf> {
    let resolved = resolve_for_containment(path)?;
    is_inside(root, &resolved).then_some(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// The temp folder as the filesystem spells it: on macOS `TempDir` hands
    /// back `/var/folders/…`, a link to `/private/var/folders/…`.
    fn resolved_root(dir: &TempDir) -> PathBuf {
        std::fs::canonicalize(dir.path()).expect("canonicalise")
    }

    #[test]
    fn a_file_in_the_folder_resolves_and_is_inside_it() {
        let dir = TempDir::new().expect("temp dir");
        let root = resolved_root(&dir);
        let note = root.join("Launch.md");
        std::fs::write(&note, "the text").expect("seed");

        assert_eq!(resolve_inside(&root, &note), Some(note.clone()));
        assert!(is_inside(&root, &note));
    }

    #[test]
    fn a_note_that_does_not_exist_yet_still_resolves_under_the_folder() {
        let dir = TempDir::new().expect("temp dir");
        let root = resolved_root(&dir);
        let unwritten = root.join("Sub").join("New.md");

        assert_eq!(resolve_inside(&root, &unwritten), Some(unwritten));
    }

    #[test]
    fn a_walk_back_out_of_the_folder_is_not_inside_it() {
        let dir = TempDir::new().expect("temp dir");
        let root = resolved_root(&dir).join("notes");
        std::fs::create_dir(&root).expect("seed");
        let outside = resolved_root(&dir).join("elsewhere.md");
        std::fs::write(&outside, "somebody else's").expect("seed");

        assert_eq!(resolve_inside(&root, &root.join("../elsewhere.md")), None);
        assert_eq!(resolve_inside(&root, &outside), None);
    }

    #[test]
    fn a_relative_path_resolves_to_nothing() {
        let dir = TempDir::new().expect("temp dir");
        let root = resolved_root(&dir);

        assert_eq!(resolve_for_containment(Path::new("Launch.md")), None);
        assert_eq!(resolve_inside(&root, Path::new("Launch.md")), None);
    }

    #[test]
    fn a_symlink_is_refused_after_resolution_not_before() {
        let dir = TempDir::new().expect("temp dir");
        let root = resolved_root(&dir).join("notes");
        std::fs::create_dir(&root).expect("seed");
        let outside = resolved_root(&dir).join("elsewhere.md");
        std::fs::write(&outside, "somebody else's").expect("seed");
        let link = root.join("Looks-Like-A-Note.md");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).expect("link");
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&outside, &link).expect("link");

        // The link itself is under the root; what it names is not.
        assert!(link.starts_with(&root));
        assert_eq!(resolve_inside(&root, &link), None);
    }

    #[test]
    fn a_sibling_folder_with_the_roots_name_as_a_prefix_is_not_inside_it() {
        let dir = TempDir::new().expect("temp dir");
        let root = resolved_root(&dir).join("Writ");
        let sibling = resolved_root(&dir).join("Writing");
        std::fs::create_dir(&root).expect("seed");
        std::fs::create_dir(&sibling).expect("seed");
        let note = sibling.join("Launch.md");
        std::fs::write(&note, "the text").expect("seed");

        assert_eq!(resolve_inside(&root, &note), None);
    }

    #[test]
    fn a_path_still_carrying_a_parent_component_is_never_inside() {
        let root = Path::new("/notes");
        assert!(!is_inside(root, Path::new("/notes/../elsewhere.md")));
    }
}
