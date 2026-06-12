use std::path::Path;

use writ_core::workspace::{is_ignored, sort_entries, WorkspaceEntry};

use crate::errors::{StorageError, StorageResult};

/// Lists the immediate children of `dir` that are inside `workspace_root`.
///
/// Both paths are canonicalized before the containment check; symlinks that
/// resolve outside `workspace_root` are rejected. Entries whose name matches
/// the default ignore set are excluded. The returned slice is sorted:
/// directories first, then files, each group ordered case-insensitively.
pub fn list_dir(workspace_root: &Path, dir: &Path) -> StorageResult<Vec<WorkspaceEntry>> {
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

    let mut entries = Vec::new();

    for entry in std::fs::read_dir(&canonical_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();

        if is_ignored(&name) {
            continue;
        }

        let entry_path = entry.path();
        let file_type = entry.file_type()?;

        let is_dir = if file_type.is_symlink() {
            let resolved = entry_path
                .canonicalize()
                .unwrap_or_else(|_| entry_path.clone());
            if !resolved.starts_with(&canonical_root) {
                continue;
            }
            resolved.is_dir()
        } else {
            file_type.is_dir()
        };

        entries.push(WorkspaceEntry {
            name,
            path: entry_path.to_string_lossy().into_owned(),
            is_dir,
        });
    }

    sort_entries(&mut entries);
    Ok(entries)
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

        let entries = list_dir(root.path(), root.path()).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"main.rs"));
        assert!(names.contains(&"src"));
    }

    #[test]
    fn dirs_come_before_files() {
        let root = setup();
        fs::write(root.path().join("aaa.rs"), "").unwrap();
        fs::create_dir(root.path().join("zzz_dir")).unwrap();

        let entries = list_dir(root.path(), root.path()).unwrap();
        assert!(entries[0].is_dir, "first entry should be a dir");
    }

    #[test]
    fn ignores_default_ignored_names() {
        let root = setup();
        fs::create_dir(root.path().join("node_modules")).unwrap();
        fs::create_dir(root.path().join(".git")).unwrap();
        fs::write(root.path().join("main.rs"), "").unwrap();

        let entries = list_dir(root.path(), root.path()).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(!names.contains(&"node_modules"));
        assert!(!names.contains(&".git"));
        assert!(names.contains(&"main.rs"));
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

        let entries = list_dir(root.path(), &sub).unwrap();
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
}
