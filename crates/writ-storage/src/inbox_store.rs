use std::path::Path;

use writ_core::inbox::{sort_inbox_files, InboxFile};
use writ_core::workspace::is_ignored;

use crate::errors::StorageResult;

/// Lists the immediate regular-file children of the watched inbox `root`.
///
/// Directories are skipped (the inbox auto-opens files, not folders) and names
/// in the default ignore set (`.DS_Store`, `.git`, …) are excluded. Each file
/// carries its byte size; a file whose metadata cannot be read is reported with
/// size `0` rather than failing the whole listing. The result is sorted
/// case-insensitively by name.
pub fn list_files(root: &Path) -> StorageResult<Vec<InboxFile>> {
    let mut files = Vec::new();

    for entry in std::fs::read_dir(root)? {
        // A single unreadable entry must not sink the whole listing; skip it.
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();

        if is_ignored(&name) {
            continue;
        }

        match entry.file_type() {
            Ok(ft) if ft.is_file() => {}
            _ => continue,
        }

        let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);

        files.push(InboxFile {
            name,
            path: entry.path().to_string_lossy().into_owned(),
            size_bytes,
        });
    }

    sort_inbox_files(&mut files);
    Ok(files)
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
    fn lists_files_with_sizes() {
        let root = setup();
        fs::write(root.path().join("report.md"), "hello").unwrap();

        let files = list_files(root.path()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "report.md");
        assert_eq!(files[0].size_bytes, 5);
        assert!(files[0].path.ends_with("report.md"));
    }

    #[test]
    fn skips_directories() {
        let root = setup();
        fs::write(root.path().join("a.md"), "").unwrap();
        fs::create_dir(root.path().join("sub")).unwrap();

        let files = list_files(root.path()).unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["a.md"]);
    }

    #[test]
    fn skips_ignored_names() {
        let root = setup();
        fs::write(root.path().join(".DS_Store"), "").unwrap();
        fs::write(root.path().join("keep.md"), "").unwrap();

        let files = list_files(root.path()).unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["keep.md"]);
    }

    #[test]
    fn sorts_case_insensitively_by_name() {
        let root = setup();
        fs::write(root.path().join("Zeta.md"), "").unwrap();
        fs::write(root.path().join("alpha.md"), "").unwrap();

        let files = list_files(root.path()).unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["alpha.md", "Zeta.md"]);
    }

    #[test]
    fn empty_folder_yields_no_files() {
        let root = setup();
        assert!(list_files(root.path()).unwrap().is_empty());
    }

    #[test]
    fn missing_folder_is_an_error() {
        assert!(list_files(Path::new("/nonexistent/xyz/abc123")).is_err());
    }
}
