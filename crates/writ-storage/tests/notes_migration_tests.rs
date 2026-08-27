//! The one-time pass that turns every note into a file (ADR-028 §4).
//!
//! One test per acceptance criterion. The fixtures build a 0.3.5-shaped
//! database by hand — rows plus the mirrors they used to be read from — so
//! each test states what the pass finds and what it leaves behind.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::notes_migration::{
    run_notes_migration, MigrationReport, MigrationRoots, RowOutcome, RECOVERED_FOLDER,
};
use writ_storage::rollback::ROLLBACK_COPY_SUFFIX;
use writ_storage::schema_meta::{self, KEY_NOTES_MIGRATION_RAN_AT, KEY_NOTES_MIGRATION_REPORT};

/// A data folder shaped the way 0.3.5 left one, plus the notes folder 0.4
/// resolves beside it.
struct Fixture {
    dir: TempDir,
    store: BufferStore,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().expect("temp dir");
        let conn = open_database(&dir.path().join("writ.db")).expect("open db");
        run_migrations(&conn).expect("migrations");
        for folder in ["buffers", "Writ", "piped"] {
            std::fs::create_dir_all(dir.path().join(folder)).expect("create folder");
        }
        let store = BufferStore::new(conn, dir.path().join("buffers"));
        Self { dir, store }
    }

    fn db_path(&self) -> PathBuf {
        self.dir.path().join("writ.db")
    }

    fn notes(&self) -> PathBuf {
        self.dir.path().join("Writ")
    }

    fn archive(&self) -> PathBuf {
        self.dir.path().join("archive")
    }

    fn piped(&self) -> PathBuf {
        self.dir.path().join("piped")
    }

    fn recovered(&self) -> PathBuf {
        self.notes().join(RECOVERED_FOLDER)
    }

    fn mirror(&self, id: &str) -> PathBuf {
        self.dir.path().join("buffers").join(format!("{id}.txt"))
    }

    /// A row with no file, holding `content` in its mirror the way 0.3.5 did.
    fn add_scratch(&self, id: &str, title: &str, content: &[u8], status: BufferStatus) {
        self.store.insert(&row(id, title, None)).expect("insert");
        std::fs::write(self.mirror(id), content).expect("write the copy");
        if status == BufferStatus::History {
            self.store.close(id).expect("close");
        }
    }

    /// A row with a file, plus the mirror 0.3.5 kept beside it.
    fn add_source_backed(&self, id: &str, name: &str, on_disk: &[u8], mirrored: &[u8]) -> PathBuf {
        let file = self.dir.path().join("outside").join(name);
        std::fs::create_dir_all(file.parent().unwrap()).expect("create folder");
        std::fs::write(&file, on_disk).expect("write the file");
        let doc = row(id, name, Some(file.to_string_lossy().into_owned()));
        self.store.insert(&doc).expect("insert");
        std::fs::write(self.mirror(id), mirrored).expect("write the copy");
        file
    }

    fn run(&self) -> MigrationReport {
        self.run_at(day(2026, 8, 28))
    }

    fn run_at(&self, now: DateTime<Utc>) -> MigrationReport {
        let db_path = self.db_path();
        let notes = self.notes();
        let archive = self.archive();
        let piped = self.piped();
        run_notes_migration(
            &self.store,
            MigrationRoots {
                db_path: &db_path,
                notes: &notes,
                archive: &archive,
                piped: &piped,
            },
            now,
        )
        .expect("the migration must not fail the launch")
    }

    fn outcome(&self, report: &MigrationReport, key: &str) -> RowOutcome {
        report
            .rows
            .iter()
            .find(|(id, _)| id == key)
            .unwrap_or_else(|| panic!("no outcome recorded for {key}: {:?}", report.rows))
            .1
            .clone()
    }
}

fn row(id: &str, title: &str, source_path: Option<String>) -> BufferDocument {
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
        created_at: day(2026, 7, 4),
        updated_at: day(2026, 7, 4),
        closed_at: None,
        read_only: false,
        size_bytes: 0,
    }
}

fn day(year: i32, month: u32, date: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, date, 12, 0, 0).unwrap()
}

/// The local calendar day of `moment`, which is what a dated file is named
/// for.
fn local_day(moment: DateTime<Utc>) -> String {
    writ_core::notes::date_stem(moment)
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

#[test]
fn four_row_shapes_produce_the_expected_file_set() {
    let fixture = Fixture::new();
    fixture.add_scratch("active-1", "Shopping", b"eggs", BufferStatus::Active);
    fixture.add_scratch(
        "history-1",
        "Old ideas",
        b"a thought",
        BufferStatus::History,
    );
    let present = fixture.add_source_backed("present-1", "kept.md", b"same", b"same");
    fixture.add_source_backed("absent-1", "lost.md", b"unreadable", b"the only text");
    std::fs::remove_file(fixture.dir.path().join("outside").join("lost.md")).unwrap();

    let report = fixture.run();

    let orphan_name = format!("lost (unsaved edits {}).md", local_day(day(2026, 8, 28)));
    assert_eq!(
        names_in(&fixture.notes()),
        HashSet::from(["Shopping.md".to_string(), RECOVERED_FOLDER.to_string()]),
        "the active note is a file beside a folder holding what could not be placed"
    );
    assert_eq!(
        names_in(&fixture.archive()),
        HashSet::from(["Old ideas.md".to_string()])
    );
    assert_eq!(names_in(&fixture.recovered()), HashSet::from([orphan_name]));
    assert_eq!(std::fs::read_to_string(&present).unwrap(), "same");
    assert!(
        names_in(fixture.store.buffers_dir()).is_empty(),
        "every copy that was placed is unlinked"
    );

    assert_eq!(report.migrated, 3, "two notes plus the orphan");
    assert_eq!(report.archived, 1);
    assert_eq!(report.failed, 0);

    // The rows now open the files.
    assert_eq!(fixture.store.read_content("active-1").unwrap(), "eggs");
    assert_eq!(fixture.store.read_content("present-1").unwrap(), "same");
    assert_eq!(
        fixture.store.read_content("absent-1").unwrap(),
        "the only text"
    );
    assert_eq!(
        fixture.store.get("history-1").unwrap().source_path,
        None,
        "an archived note stays out of the notes folder until the user moves it"
    );
}

#[test]
fn rerunning_the_migration_produces_no_additional_files() {
    let fixture = Fixture::new();
    fixture.add_scratch("active-1", "Shopping", b"eggs", BufferStatus::Active);
    fixture.add_scratch(
        "history-1",
        "Old ideas",
        b"a thought",
        BufferStatus::History,
    );
    fixture.add_source_backed("present-1", "kept.md", b"same", b"same");

    let first = fixture.run();
    let notes_after_first = names_in(&fixture.notes());
    let archive_after_first = names_in(&fixture.archive());

    let second = fixture.run_at(day(2026, 9, 1));

    assert_eq!(names_in(&fixture.notes()), notes_after_first);
    assert_eq!(names_in(&fixture.archive()), archive_after_first);
    assert_eq!(
        second, first,
        "a settled run returns the report it stored and changes nothing"
    );
}

#[test]
fn divergent_mirror_writes_a_recovered_file_and_leaves_the_source_byte_identical() {
    let fixture = Fixture::new();
    let file = fixture.add_source_backed(
        "diverged-1",
        "notes.md",
        b"what the file has",
        b"what the editor had",
    );

    let report = fixture.run();

    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "what the file has",
        "the file the user opened is never rewritten"
    );
    let recovered = fixture.recovered().join(format!(
        "notes (unsaved edits {}).md",
        local_day(day(2026, 8, 28))
    ));
    assert_eq!(
        std::fs::read_to_string(&recovered).unwrap(),
        "what the editor had"
    );
    assert_eq!(report.recovered, 1);
    assert_eq!(
        fixture.outcome(&report, "diverged-1"),
        RowOutcome::RecoveredUnsavedEdits {
            source: file.to_string_lossy().into_owned(),
            recovered: recovered.to_string_lossy().into_owned(),
        }
    );
    assert!(!fixture.mirror("diverged-1").exists());
}

#[cfg(unix)]
#[test]
fn a_row_that_fails_verification_keeps_its_mirror_and_appears_in_the_report() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    fixture.add_scratch(
        "blocked-1",
        "Refused",
        b"text worth keeping",
        BufferStatus::Active,
    );

    // A notes folder nothing can be created in: the write fails, so nothing
    // may be deleted on the strength of it.
    let notes = fixture.notes();
    std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o500)).unwrap();

    let report = fixture.run();

    std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(report.failed, 1);
    assert_eq!(report.migrated, 0);
    assert!(
        matches!(
            fixture.outcome(&report, "blocked-1"),
            RowOutcome::VerificationFailed { .. }
        ),
        "got {:?}",
        fixture.outcome(&report, "blocked-1")
    );
    assert_eq!(
        std::fs::read_to_string(fixture.mirror("blocked-1")).unwrap(),
        "text worth keeping",
        "a write that did not verify never licenses a delete"
    );
    assert!(fixture.store.get("blocked-1").is_ok(), "the row survives");
}

#[test]
fn history_rows_land_in_the_archive_and_nothing_is_written_to_the_notes_folder_for_them() {
    let fixture = Fixture::new();
    fixture.add_scratch("history-1", "Closed one", b"first", BufferStatus::History);
    fixture.add_scratch("history-2", "Closed two", b"second", BufferStatus::History);

    let report = fixture.run();

    assert_eq!(
        names_in(&fixture.archive()),
        HashSet::from(["Closed one.md".to_string(), "Closed two.md".to_string()])
    );
    assert!(
        names_in(&fixture.notes()).is_empty(),
        "nothing is uploaded to a folder that may be syncing before the user agrees"
    );
    assert_eq!(report.archived, 2);
    assert_eq!(report.migrated, 0);
}

#[test]
fn two_rows_titled_notes_produce_notes_md_and_notes_2_md() {
    let fixture = Fixture::new();
    fixture.add_scratch("a", "Notes", b"first", BufferStatus::Active);
    fixture.add_scratch("b", "Notes", b"second", BufferStatus::Active);

    fixture.run();

    assert_eq!(
        names_in(&fixture.notes()),
        HashSet::from(["Notes.md".to_string(), "Notes 2.md".to_string()])
    );
    let first = std::fs::read_to_string(fixture.notes().join("Notes.md")).unwrap();
    let second = std::fs::read_to_string(fixture.notes().join("Notes 2.md")).unwrap();
    assert_eq!(
        HashSet::from([first, second]),
        HashSet::from(["first".to_string(), "second".to_string()]),
        "neither note is written over the other"
    );
}

#[test]
fn a_row_titled_writ_4_produces_a_date_named_file() {
    let fixture = Fixture::new();
    fixture.add_scratch("minted-1", "writ-4", b"typed once", BufferStatus::Active);
    fixture.add_scratch("minted-2", "", b"typed twice", BufferStatus::Active);

    fixture.run();

    // Both rows were created on the same day, so the second dedupes.
    let created = local_day(day(2026, 7, 4));
    assert_eq!(
        names_in(&fixture.notes()),
        HashSet::from([format!("{created}.md"), format!("{created} 2.md")]),
        "a title Writ minted names nothing, so the note takes its date"
    );
}

#[test]
fn empty_rows_are_deleted_and_write_no_file() {
    let fixture = Fixture::new();
    fixture.add_scratch("empty-1", "writ-1", b"", BufferStatus::Active);
    let doc = row("empty-2", "writ-2", None);
    fixture.store.insert(&doc).expect("insert");

    let report = fixture.run();

    assert!(names_in(&fixture.notes()).is_empty());
    assert!(names_in(&fixture.archive()).is_empty());
    assert_eq!(report.deleted_empty, 2);
    assert!(fixture.store.get("empty-1").is_err());
    assert!(fixture.store.get("empty-2").is_err());
}

#[test]
fn files_of_piped_input_become_notes_and_the_folder_is_left_empty() {
    let fixture = Fixture::new();
    let piped = fixture.piped().join("build-log.txt");
    std::fs::write(&piped, b"cargo output").unwrap();

    // The CLI wrote the file and then asked Writ to open it, so a row points
    // at it: moving it has to take the row along.
    let doc = row(
        "opened-piped",
        "build-log.txt",
        Some(piped.to_string_lossy().into_owned()),
    );
    fixture.store.insert(&doc).expect("insert");

    let report = fixture.run();

    assert!(!piped.exists(), "the folder is left empty");
    assert_eq!(
        std::fs::read_to_string(fixture.notes().join("build-log.md")).unwrap(),
        "cargo output"
    );
    assert_eq!(report.piped, 1);
    assert_eq!(
        fixture.store.read_content("opened-piped").unwrap(),
        "cargo output",
        "the row opens the note it became"
    );
}

#[test]
fn a_file_of_piped_input_that_no_row_opened_still_becomes_a_note() {
    let fixture = Fixture::new();
    std::fs::write(fixture.piped().join("stray.txt"), b"nobody opened this").unwrap();

    let report = fixture.run();

    assert!(names_in(&fixture.piped()).is_empty());
    assert_eq!(
        std::fs::read_to_string(fixture.notes().join("stray.md")).unwrap(),
        "nobody opened this"
    );
    assert_eq!(
        fixture.outcome(&report, "stray.txt"),
        RowOutcome::PipedFile {
            from: fixture
                .piped()
                .join("stray.txt")
                .to_string_lossy()
                .into_owned(),
            path: fixture
                .notes()
                .join("stray.md")
                .to_string_lossy()
                .into_owned(),
        }
    );
}

#[test]
fn rollback_copy_exists_and_schema_meta_names_it() {
    let fixture = Fixture::new();
    fixture.add_scratch("active-1", "Shopping", b"eggs", BufferStatus::Active);

    fixture.run();

    let copy = fixture
        .dir
        .path()
        .join(format!("writ.db{ROLLBACK_COPY_SUFFIX}"));
    assert!(
        copy.exists(),
        "the database is copied before the first write"
    );

    let conn = open_database(&fixture.db_path()).expect("read the meta rows");
    assert_eq!(
        schema_meta::get(&conn, "notes_migration_rollback_path")
            .unwrap()
            .as_deref(),
        copy.to_str()
    );
    assert!(schema_meta::get(&conn, KEY_NOTES_MIGRATION_RAN_AT)
        .unwrap()
        .is_some());
    let stored: MigrationReport = serde_json::from_str(
        &schema_meta::get(&conn, KEY_NOTES_MIGRATION_REPORT)
            .unwrap()
            .unwrap(),
    )
    .expect("the report is stored as JSON");
    assert_eq!(stored.migrated, 1);
}

#[test]
fn a_launch_with_nothing_to_migrate_leaves_no_copy_of_the_database() {
    let fixture = Fixture::new();

    let report = fixture.run();

    assert!(
        !fixture
            .dir
            .path()
            .join(format!("writ.db{ROLLBACK_COPY_SUFFIX}"))
            .exists(),
        "a copy is taken to protect work, and there is none to protect"
    );
    assert_eq!(report.rows, Vec::new());
    let conn = open_database(&fixture.db_path()).unwrap();
    assert!(
        schema_meta::get(&conn, KEY_NOTES_MIGRATION_RAN_AT)
            .unwrap()
            .is_some(),
        "the run is still recorded, so the next launch skips it"
    );
}

#[test]
fn deleting_the_database_and_reopening_leaves_every_note_openable() {
    let fixture = Fixture::new();
    fixture.add_scratch("active-1", "Shopping", b"eggs", BufferStatus::Active);
    fixture.add_scratch("active-2", "Ideas", b"a thought", BufferStatus::Active);
    fixture.run();

    let notes = names_in(&fixture.notes());
    let bodies: Vec<String> = notes
        .iter()
        .map(|name| std::fs::read_to_string(fixture.notes().join(name)).unwrap())
        .collect();

    // Every database file goes, which is the promise: the index and the
    // session layout are what it cost, and no note is among them.
    for suffix in ["", "-wal", "-shm", ROLLBACK_COPY_SUFFIX] {
        let _ = std::fs::remove_file(fixture.dir.path().join(format!("writ.db{suffix}")));
    }
    let conn = open_database(&fixture.db_path()).expect("reopen");
    run_migrations(&conn).expect("migrations on a fresh database");

    assert_eq!(names_in(&fixture.notes()), notes);
    assert_eq!(
        bodies.iter().cloned().collect::<HashSet<_>>(),
        HashSet::from(["eggs".to_string(), "a thought".to_string()])
    );
}

#[test]
fn a_row_whose_recorded_file_went_missing_is_written_again() {
    let fixture = Fixture::new();
    fixture.add_scratch("active-1", "Shopping", b"eggs", BufferStatus::Active);
    fixture.run();

    let file = fixture.notes().join("Shopping.md");
    std::fs::remove_file(&file).unwrap();
    // The mirror is gone too, so all the pass can do is repoint the row at the
    // file it named; nothing is invented.
    let report = fixture.run_at(day(2026, 9, 1));

    assert!(
        matches!(
            fixture.outcome(&report, "active-1"),
            RowOutcome::AlreadyOnDisk { .. }
        ),
        "the row keeps naming its file so the next save recreates it: {:?}",
        fixture.outcome(&report, "active-1")
    );
}

#[cfg(unix)]
#[test]
fn a_row_that_failed_verification_is_tried_again_on_the_next_launch() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    fixture.add_scratch(
        "blocked-1",
        "Refused",
        b"text worth keeping",
        BufferStatus::Active,
    );

    let notes = fixture.notes();
    std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o500)).unwrap();
    assert_eq!(fixture.run().failed, 1);
    std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o755)).unwrap();

    let second = fixture.run_at(day(2026, 9, 1));

    assert_eq!(second.failed, 0);
    assert_eq!(second.migrated, 1);
    assert_eq!(
        std::fs::read_to_string(fixture.notes().join("Refused.md")).unwrap(),
        "text worth keeping",
        "a copy left behind is what a retry is for"
    );
    assert!(!fixture.mirror("blocked-1").exists());
}
