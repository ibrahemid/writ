use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;
use writ_core::buffer::manager::BufferManager;
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::notes_index::{self, NotesIndexStore};

fn fixture() -> (TempDir, PathBuf, PathBuf) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
    drop(conn);
    let notes = dir.path().join("Writ");
    std::fs::create_dir_all(&notes).expect("create notes dir");
    (dir, db_path, notes)
}

fn write(notes: &Path, rel: &str, body: &str) -> PathBuf {
    let path = notes.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    std::fs::write(&path, body).expect("write");
    path
}

fn key(path: &Path) -> String {
    notes_index::index_key(path)
}

fn buffer_store(
    dir: &TempDir,
    db_path: &Path,
    notes: &Path,
    index: &Arc<NotesIndexStore>,
) -> BufferStore {
    let buffers = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers).expect("buffers");
    let mut store = BufferStore::new(open_database(db_path).expect("store db"), buffers);
    store.set_notes_root(notes.to_path_buf());
    store.set_notes_index(Arc::clone(index));
    store
}

#[test]
fn a_file_under_a_folder_the_walk_skips_gets_no_rows_from_an_open_a_save_or_a_reindex() {
    let (dir, db_path, notes) = fixture();
    let old = write(&notes, ".stversions/Plan~old.md", "# Old plan\n\n#plan\n");
    let kept = write(&notes, "vendor/Plan.md", "# Vendor plan\n\n#plan\n");
    let index = Arc::new(NotesIndexStore::open(&db_path).expect("index"));
    let store = buffer_store(&dir, &db_path, &notes, &index);

    let mut manager = BufferManager::new();
    for path in [&old, &kept] {
        let text = std::fs::read_to_string(path).expect("read");
        let doc = manager
            .open_external(path.to_string_lossy().into_owned())
            .expect("open");
        store.open_from_path(&doc, &text).expect("open_from_path");
        store
            .save_content(&doc.id, &format!("{text}\n## Later\n"))
            .expect("save");
        store.reindex_buffer(&doc.id).expect("reindex");
    }

    assert_eq!(
        index.paths_for_tag("plan").expect("tag"),
        vec![key(&kept)],
        "a tag counts only files the notes tree shows"
    );
    assert_eq!(index.note_paths().expect("rows"), vec![key(&kept)]);
}

#[test]
fn the_walk_rule_judges_only_the_names_below_the_notes_folder() {
    let dir = TempDir::new().expect("tempdir");
    let notes = dir.path().join(".trash").join("Writ");
    let plan = write(&notes, "Plan.md", "#plan\n");
    let kept = write(&notes, "vendor/build/Plan.md", "#plan\n");
    let versioned = write(&notes, ".stversions/Plan~old.md", "#plan\n");
    let template = write(&notes, "Projects/.obsidian/templates/Plan.md", "#plan\n");
    let backup = write(&notes, "Plan.md~", "#plan\n");
    let outside = write(dir.path(), ".stversions/Plan.md", "#plan\n");

    assert!(!notes_index::notes_walk_skips(&notes, &plan));
    assert!(!notes_index::notes_walk_skips(&notes, &kept));
    assert!(notes_index::notes_walk_skips(&notes, &versioned));
    assert!(notes_index::notes_walk_skips(&notes, &template));
    assert!(notes_index::notes_walk_skips(&notes, &backup));
    assert!(!notes_index::notes_walk_skips(&notes, &outside));
}

#[cfg(unix)]
#[test]
fn the_walk_rule_reads_a_notes_folder_reached_through_a_link_as_its_target() {
    let dir = TempDir::new().expect("tempdir");
    let notes = dir.path().join("Writ");
    let versioned = write(&notes, ".stversions/Plan~old.md", "#plan\n");
    let plan = write(&notes, "Plan.md", "#plan\n");
    let link = dir.path().join("Linked");
    std::os::unix::fs::symlink(&notes, &link).expect("symlink");

    assert!(notes_index::notes_walk_skips(&link, &versioned));
    assert!(!notes_index::notes_walk_skips(&link, &plan));
}
