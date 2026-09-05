//! Startup over a 0.3.5-shaped data folder (ADR-028 §4).
//!
//! The storage tests drive the pass directly; this one drives the launch.
//! `AppState::initialize` resolves the notes folder, recovers, migrates, ages
//! the rollback copy and reclaims, in that order, and the order is what the
//! assertions here are about: nothing the migration is going to write may be
//! reclaimed first, and nothing may be deleted before its file verified.
//!
//! One test in its own binary, because `initialize` reads `WRIT_DATA_DIR` and
//! `WRIT_NOTES_DIR` from the process environment and a second test in the same
//! binary would race it.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::Utc;
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::notes_migration::{MigrationReport, RECOVERED_FOLDER};
use writ_storage::rollback::ROLLBACK_COPY_SUFFIX;
use writ_storage::schema_meta::{self, KEY_NOTES_MIGRATION_REPORT};
use writ_tauri_lib::state::AppState;

fn row(id: &str, title: &str, source_path: Option<String>) -> BufferDocument {
    let now = Utc::now();
    BufferDocument {
        id: id.to_string(),
        title: title.to_string(),
        filename: format!("{id}.txt"),
        status: BufferStatus::Active,
        language: None,
        source_path,
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: 0,
        created_at: now,
        updated_at: now,
        closed_at: None,
        read_only: false,
        size_bytes: 0,
        line_ending: writ_core::notes::line_ending::LineEnding::Lf,
    }
}

fn names_in(dir: &Path) -> HashSet<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Writes the four row shapes plus a file of piped input into `writ_dir`,
/// exactly as 0.3.5 would have left them.
fn seed_a_0_3_5_data_folder(writ_dir: &Path) -> PathBuf {
    let buffers = writ_dir.join("buffers");
    std::fs::create_dir_all(&buffers).expect("mirror folder");
    std::fs::create_dir_all(writ_dir.join("piped")).expect("piped folder");

    let conn = open_database(&writ_dir.join("writ.db")).expect("open db");
    run_migrations(&conn).expect("migrations");
    let store = BufferStore::new(conn, buffers.clone());

    // 1. An active note that never had a file.
    store.insert(&row("active-1", "Shopping", None)).unwrap();
    std::fs::write(buffers.join("active-1.txt"), b"eggs and milk").unwrap();

    // 2. A closed one, likewise.
    store.insert(&row("history-1", "Old ideas", None)).unwrap();
    std::fs::write(buffers.join("history-1.txt"), b"a thought").unwrap();
    store.close("history-1").unwrap();

    // 3. A file opened from outside, whose copy agrees with it.
    let outside = writ_dir.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let kept = outside.join("kept.md");
    std::fs::write(&kept, b"# Kept").unwrap();
    store
        .insert(&row(
            "present-1",
            "kept.md",
            Some(kept.to_string_lossy().into_owned()),
        ))
        .unwrap();
    std::fs::write(buffers.join("present-1.txt"), b"# Kept").unwrap();

    // 4. One whose copy holds edits the file never received — the population
    //    the save defect in 0.3.0 through 0.3.2 produced.
    let diverged = outside.join("diverged.md");
    std::fs::write(&diverged, b"what the file has").unwrap();
    store
        .insert(&row(
            "diverged-1",
            "diverged.md",
            Some(diverged.to_string_lossy().into_owned()),
        ))
        .unwrap();
    std::fs::write(buffers.join("diverged-1.txt"), b"what the editor had").unwrap();

    // An empty row, which is deleted rather than written out.
    store.insert(&row("empty-1", "writ-17", None)).unwrap();
    std::fs::write(buffers.join("empty-1.txt"), b"").unwrap();

    // A file the CLI wrote from piped input.
    std::fs::write(
        writ_dir.join("piped").join("build-log.txt"),
        b"cargo output",
    )
    .unwrap();

    kept
}

#[test]
fn a_launch_over_an_0_3_5_data_folder_turns_every_note_into_a_file() {
    let data = TempDir::new().expect("data folder");
    let notes = TempDir::new().expect("notes folder");
    let kept = seed_a_0_3_5_data_folder(data.path());

    std::env::set_var("WRIT_DATA_DIR", data.path());
    std::env::set_var("WRIT_NOTES_DIR", notes.path());
    let state = AppState::initialize().expect("the launch must not fail");
    std::env::remove_var("WRIT_DATA_DIR");
    std::env::remove_var("WRIT_NOTES_DIR");

    let notes_root = state.notes_root().clone();
    assert_eq!(
        notes_root,
        writ_tauri_lib::security::canonicalize_root(notes.path()).unwrap()
    );

    // The file set.
    assert_eq!(
        names_in(&notes_root),
        HashSet::from([
            "Shopping.md".to_string(),
            "build-log.md".to_string(),
            RECOVERED_FOLDER.to_string(),
        ]),
        "one file per note the user can see, plus what could not be placed"
    );
    assert_eq!(
        names_in(&notes_root.join(RECOVERED_FOLDER)).len(),
        1,
        "the copy that disagreed with its file is written beside it"
    );
    assert_eq!(
        names_in(&data.path().join("archive")),
        HashSet::from(["Old ideas.md".to_string()]),
        "a closed note waits under Writ's own folder until the user moves it"
    );
    assert_eq!(
        std::fs::read_to_string(&kept).unwrap(),
        "# Kept",
        "a file Writ only opened is never rewritten"
    );
    assert_eq!(
        std::fs::read_to_string(data.path().join("outside").join("diverged.md")).unwrap(),
        "what the file has"
    );
    assert!(
        names_in(&data.path().join("buffers")).is_empty(),
        "every copy is unlinked once the file that replaces it verified"
    );
    assert!(
        names_in(&data.path().join("piped")).is_empty(),
        "the piped folder is not kept as an exception"
    );

    // The rows.
    {
        let store = state.store.lock().unwrap();
        assert_eq!(store.read_content("active-1").unwrap(), "eggs and milk");
        assert_eq!(store.read_content("present-1").unwrap(), "# Kept");
        assert_eq!(
            store.read_content("diverged-1").unwrap(),
            "what the file has"
        );
        assert!(
            store.get("empty-1").is_err(),
            "a row holding nothing is deleted, not archived as a blank file"
        );
        assert!(
            store.get("history-1").is_ok(),
            "reclaim must not delete the row the migration just archived"
        );
    }

    // The report.
    let conn = open_database(&data.path().join("writ.db")).unwrap();
    let stored: MigrationReport = serde_json::from_str(
        &schema_meta::get(&conn, KEY_NOTES_MIGRATION_REPORT)
            .unwrap()
            .expect("a report is stored for the panel to render"),
    )
    .expect("the report is JSON");
    assert_eq!(stored.migrated, 2, "the active note and the file it opened");
    assert_eq!(stored.archived, 1);
    assert_eq!(stored.recovered, 1);
    assert_eq!(stored.piped, 1);
    assert_eq!(stored.deleted_empty, 1);
    assert_eq!(stored.failed, 0);
    assert_eq!(stored.notes_folder, notes_root.to_string_lossy());

    // The rollback copy, and the launch that took it counting itself.
    let copy = data.path().join(format!("writ.db{ROLLBACK_COPY_SUFFIX}"));
    assert!(
        copy.exists(),
        "the database is copied before the first write"
    );
    assert_eq!(
        schema_meta::get(&conn, "notes_migration_rollback_launches")
            .unwrap()
            .as_deref(),
        Some("1"),
        "the launch that took the copy is one of the ten it survives"
    );
}
