//! Which files in the notes folder the index holds (ADR-028 section 7).
//!
//! The notes folder is writing, not a code checkout: a folder named `vendor`,
//! `build` or `coverage` holds notes like any other, and a `.gitignore` in the
//! folder says what to commit rather than what exists. The walk leaves out
//! `.git`, the folders another client keeps for itself and temp files, and
//! nothing else.

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use writ_core::notes::links::Resolution;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::notes_index::{self, NotesIndexStore};

fn fixture() -> (TempDir, PathBuf, NotesIndexStore) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
    drop(conn);
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("create notes dir");
    let index = NotesIndexStore::open(&db_path).expect("index");
    (dir, notes, index)
}

fn write(notes: &Path, rel: &str, body: &str) -> PathBuf {
    let path = notes.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    std::fs::write(&path, body).expect("write");
    path
}

fn walk(index: &NotesIndexStore, notes: &Path) {
    let outcome = index
        .reconcile(notes, &|| false, &|_| false)
        .expect("reconcile");
    assert!(!outcome.cancelled);
}

fn search(index: &NotesIndexStore, raw: &str) -> Vec<String> {
    let query = writ_core::search::to_prefix_match(raw).expect("query");
    let terms = writ_core::search::search_terms(raw);
    index
        .search_hits(&query, &terms, 50)
        .expect("search")
        .into_iter()
        .filter_map(|hit| hit.path)
        .collect()
}

#[test]
fn notes_under_vendor_build_and_coverage_are_indexed_searched_and_linked() {
    let (_dir, notes, index) = fixture();
    let acme = write(&notes, "vendor/Acme.md", "acme supplier terms\n");
    let plan = write(&notes, "build/Plan.md", "build plan for the shed\n");
    let report = write(&notes, "coverage/Report.md", "insurance coverage report\n");
    let home = write(
        &notes,
        "Home.md",
        "See [[Acme]], [[Plan]] and [[Report]].\n",
    );
    walk(&index, &notes);

    let home_key = notes_index::index_key(&home);
    for (note, word, name) in [
        (&acme, "supplier", "Acme"),
        (&plan, "shed", "Plan"),
        (&report, "insurance", "Report"),
    ] {
        let key = notes_index::index_key(note);
        assert_eq!(search(&index, word), vec![key.clone()], "search for {word}");
        assert_eq!(
            index.resolve_link(&home_key, name).expect("resolve"),
            Resolution::Resolved(key.clone()),
            "[[{name}]]"
        );
        let backlinks = index.backlinks(&key).expect("backlinks");
        assert_eq!(
            backlinks
                .iter()
                .map(|row| row.from_path.as_str())
                .collect::<Vec<_>>(),
            vec![home_key.as_str()],
            "backlinks of {name}"
        );
        assert!(
            index
                .search_names(name, &notes, 10)
                .expect("names")
                .iter()
                .any(|hit| hit.path == key),
            "quick open finds {name}"
        );
    }
}

#[test]
fn a_gitignore_inside_the_notes_folder_hides_nothing() {
    let (_dir, notes, index) = fixture();
    write(&notes, ".gitignore", "*.md\nscratch/\nvendor/\n");
    let idea = write(&notes, "scratch/idea.md", "a passing idea\n");
    let note = write(&notes, "Plain.md", "a plain note\n");
    walk(&index, &notes);

    assert_eq!(
        search(&index, "passing"),
        vec![notes_index::index_key(&idea)]
    );
    assert_eq!(search(&index, "plain"), vec![notes_index::index_key(&note)]);
}

#[test]
fn what_another_tool_keeps_for_itself_and_temp_files_stay_out() {
    let (_dir, notes, index) = fixture();
    write(&notes, ".git/notes.md", "git internals\n");
    write(&notes, ".obsidian/workspace.md", "obsidian settings\n");
    write(&notes, ".trash/old.md", "thrown away\n");
    write(&notes, ".stfolder/marker.md", "syncthing marker\n");
    write(&notes, ".stversions/old~20260101.md", "syncthing version\n");
    write(&notes, "Report.md.crdownload", "half a download\n");
    write(&notes, ".writ-tmp-a1b2c3", "a save in flight\n");
    write(&notes, ".DS_Store", "finder view settings\n");
    write(&notes, "vendor/.DS_Store", "finder view settings\n");
    let kept = write(&notes, "Kept.md", "kept\n");
    walk(&index, &notes);

    let indexed: Vec<String> = index
        .snapshot()
        .expect("snapshot")
        .into_iter()
        .map(|(path, ..)| path)
        .collect();
    assert_eq!(indexed, vec![notes_index::index_key(&kept)]);
}

/// The watcher's arm: a note saved under `vendor/` by another editor is
/// indexed by its path like any other.
#[test]
fn a_note_under_a_build_folder_is_indexed_by_the_watcher_arm() {
    let (_dir, notes, index) = fixture();
    walk(&index, &notes);
    let acme = write(&notes, "vendor/Acme.md", "acme supplier terms\n");

    assert!(index.index_path(&acme).expect("index_path"));
    assert_eq!(
        search(&index, "supplier"),
        vec![notes_index::index_key(&acme)]
    );
}
