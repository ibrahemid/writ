//! The notes folder as the sidebar's tree lists it: `workspace_store::list_notes_dir`.
//!
//! The tree a tag filters is this listing, and the files a tag names come from
//! the notes index. A file the index holds and the tree hides would match the
//! tag and show nowhere, so what is tested here is that the two agree: the
//! listing hides what the index's walk skips, and nothing else.

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use writ_storage::notes_index::{self, NotesIndexStore};
use writ_storage::workspace_store;

/// A notes folder holding `files`, and an index over it, walked once.
fn walked(files: &[&str]) -> (TempDir, PathBuf, NotesIndexStore) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = writ_storage::database::connection::open_database(&db_path).expect("open_database");
    writ_storage::database::migrations::run_migrations(&conn).expect("migrations");
    drop(conn);

    let root = dir.path().join("Writ");
    std::fs::create_dir_all(&root).expect("notes folder");
    let root = root.canonicalize().expect("canonical notes folder");
    for relative in files {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
        std::fs::write(&path, "# Text\n\n#plan\n").expect("write");
    }

    let index = NotesIndexStore::open(&db_path).expect("index");
    index.reconcile(&root, &|| false, &|_| false).expect("walk");
    (dir, root, index)
}

/// Every file the tree shows, opening each folder it lists, in index spelling.
fn every_listed_file(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in workspace_store::list_notes_dir(root, &folder)
            .expect("listing")
            .entries
        {
            if entry.is_dir {
                folders.push(PathBuf::from(entry.path));
            } else {
                files.push(notes_index::index_key(Path::new(&entry.path)));
            }
        }
    }
    files.sort();
    files
}

#[test]
fn the_tree_shows_every_file_the_index_holds_and_none_it_skips() {
    let (_dir, root, index) = walked(&[
        "Plan.md",
        "Projects/Launch.md",
        "Projects/.obsidian/cache.md",
        "build/Plan.md",
        "vendor/Acme.md",
        ".obsidian/workspace.md",
        ".git/notes.md",
        ".DS_Store",
        ".writ-tmp-a1b2c3",
        "Report.md.crdownload",
    ]);

    let mut held = index.note_paths().expect("index rows");
    held.sort();
    assert!(
        held.contains(&notes_index::index_key(
            &root.join("Projects").join("Launch.md")
        )),
        "the walk reads folders: {held:?}"
    );

    assert_eq!(
        every_listed_file(&root),
        held,
        "the tree hides what the index skips by name, and nothing else"
    );
}

#[test]
fn a_name_the_index_reads_stays_in_the_tree() {
    let (_dir, root, _index) = walked(&["a.md", "Plan.md", "notes.txt", "README", "photo.png"]);
    std::fs::create_dir(root.join("Projects")).expect("folder");

    let names: Vec<String> = workspace_store::list_notes_dir(&root, &root)
        .expect("listing")
        .entries
        .into_iter()
        .map(|entry| entry.name)
        .collect();

    assert_eq!(
        names,
        vec![
            "Projects",
            "a.md",
            "notes.txt",
            "photo.png",
            "Plan.md",
            "README"
        ]
    );
}

#[test]
fn a_folder_outside_the_notes_folder_is_refused() {
    let (dir, root, _index) = walked(&["Plan.md"]);
    let outside = dir.path().join("elsewhere");
    std::fs::create_dir_all(&outside).expect("folder");
    std::fs::write(outside.join("Secret.md"), "# Secret\n").expect("write");

    assert!(workspace_store::list_notes_dir(&root, &outside).is_err());
    assert!(workspace_store::list_notes_dir(&root, &root.join("..")).is_err());
}
