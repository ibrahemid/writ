//! A developer's global gitignore hides no note (ADR-028 section 7).
//!
//! One test in its own binary: it points `HOME`, `USERPROFILE` and
//! `XDG_CONFIG_HOME` at a folder holding a global gitignore, which is process
//! state no other test may see. Windows finds the home folder through
//! `USERPROFILE`, so without it a `.gitconfig` on the machine running the test
//! could name another global ignore file.

use tempfile::TempDir;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::notes_index::{self, NotesIndexStore};
use writ_storage::workspace_search::walk_index;

#[test]
fn a_global_gitignore_hides_no_note() {
    let home = TempDir::new().expect("home");
    std::fs::create_dir_all(home.path().join("git")).expect("config dir");
    std::fs::write(home.path().join("git").join("ignore"), "*.log\nscratch/\n").expect("ignore");
    std::env::set_var("HOME", home.path());
    std::env::set_var("USERPROFILE", home.path());
    std::env::set_var("XDG_CONFIG_HOME", home.path());

    let dir = TempDir::new().expect("tempdir");
    let notes = dir.path().join("notes");
    let idea = notes.join("scratch").join("idea.md");
    let log = notes.join("standup.log");
    std::fs::create_dir_all(idea.parent().expect("parent")).expect("mkdir");
    std::fs::write(&idea, "an idea worth keeping\n").expect("write");
    std::fs::write(&log, "monday standup notes\n").expect("write");

    // The workspace walk honours the global gitignore, which shows the
    // environment above is one the walk reads.
    let workspace = walk_index(&notes, 100);
    assert!(
        workspace.paths.is_empty(),
        "the global gitignore was not read: {:?}",
        workspace.paths
    );

    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
    drop(conn);
    let index = NotesIndexStore::open(&db_path).expect("index");
    index
        .reconcile(&notes, &|| false, &|_| false)
        .expect("reconcile");

    let mut indexed: Vec<String> = index
        .snapshot()
        .expect("snapshot")
        .into_iter()
        .map(|(path, ..)| path)
        .collect();
    indexed.sort();
    let mut expected = vec![notes_index::index_key(&idea), notes_index::index_key(&log)];
    expected.sort();
    assert_eq!(indexed, expected);
}
