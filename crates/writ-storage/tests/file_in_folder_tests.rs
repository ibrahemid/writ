//! A `writ://` link's path, resolved against the notes folder (ADR-052).

use std::fs;

use tempfile::TempDir;
use writ_storage::paths::file_in_folder;

fn folder() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().expect("temp dir");
    let root = fs::canonicalize(dir.path())
        .expect("canonical root")
        .join("Notes");
    fs::create_dir_all(root.join("Projects")).expect("folders");
    fs::write(root.join("Plan.md"), "plan").expect("note");
    fs::write(root.join("Projects/Launch Plan.md"), "launch").expect("note");
    fs::write(dir.path().join("secret.md"), "outside").expect("outside");
    (dir, root)
}

#[test]
fn a_file_the_folder_holds_resolves_to_it() {
    let (_dir, root) = folder();
    assert_eq!(file_in_folder(&root, "Plan.md"), Some(root.join("Plan.md")));
    assert_eq!(
        file_in_folder(&root, "Projects/Launch Plan.md"),
        Some(root.join("Projects").join("Launch Plan.md"))
    );
}

#[test]
fn a_path_out_of_the_folder_resolves_to_nothing() {
    let (_dir, root) = folder();
    for relative in ["../secret.md", "Projects/../../secret.md", "/etc/passwd"] {
        assert_eq!(file_in_folder(&root, relative), None, "{relative}");
    }
}

#[test]
fn a_folder_or_a_missing_file_resolves_to_nothing() {
    let (_dir, root) = folder();
    assert_eq!(file_in_folder(&root, "Projects"), None);
    assert_eq!(file_in_folder(&root, "Missing.md"), None);
}

#[cfg(unix)]
#[test]
fn a_symlink_out_of_the_folder_resolves_to_nothing() {
    let (dir, root) = folder();
    std::os::unix::fs::symlink(dir.path().join("secret.md"), root.join("Link.md"))
        .expect("symlink");
    assert_eq!(file_in_folder(&root, "Link.md"), None);
}
