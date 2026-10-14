//! Search in Arabic, CJK and accented text: the folded index (migration 051)
//! and the query plan that routes a search to it.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use tempfile::TempDir;
use writ_core::search::{plan_search, SearchHit};
use writ_storage::database::connection::open_database;
use writ_storage::database::index_repair::{repair_notes_index, IndexRepairOutcome};
use writ_storage::database::migrations::run_migrations;
use writ_storage::notes_index::{self, NotesIndex};

fn fixture() -> (TempDir, Connection, PathBuf) {
    let dir = TempDir::new().expect("tempdir");
    let conn = open_database(&dir.path().join("writ.db")).expect("open_database");
    run_migrations(&conn).expect("migrations");
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("create notes dir");
    (dir, conn, notes)
}

fn indexed(conn: &Connection, notes: &Path, files: &[(&str, &str)]) {
    for (name, body) in files {
        std::fs::write(notes.join(name), body).expect("write note");
    }
    notes_index::reconcile(conn, notes, &|| false, &|_| false).expect("reconcile");
}

fn hits(conn: &Connection, raw: &str) -> Vec<SearchHit> {
    let query = plan_search(raw).expect("query");
    let index = NotesIndex::new(conn);
    let hits = index.search(&query, 50).expect("search");
    assert_eq!(
        index.search_count(&query).expect("count"),
        hits.len(),
        "the count and the hits disagree for {raw}"
    );
    hits
}

fn names(conn: &Connection, raw: &str) -> Vec<String> {
    let mut names: Vec<String> = hits(conn, raw).into_iter().map(|hit| hit.title).collect();
    names.sort();
    names
}

fn matched(hit: &SearchHit) -> Vec<String> {
    hit.snippet
        .iter()
        .filter(|segment| segment.matched)
        .map(|segment| segment.text.clone())
        .collect()
}

#[test]
fn a_query_without_harakat_finds_a_note_written_with_them() {
    let (_dir, conn, notes) = fixture();
    indexed(
        &conn,
        &notes,
        &[
            ("with.md", "هذه مُلاحَظةٌ مهمة\n"),
            ("without.md", "وهذه ملاحظة أخرى\n"),
            ("other.md", "nothing here\n"),
        ],
    );

    assert_eq!(names(&conn, "ملاحظة"), vec!["with.md", "without.md"]);
    assert_eq!(names(&conn, "مُلاحَظةٌ"), vec!["with.md", "without.md"]);
}

#[test]
fn letter_variants_find_each_other() {
    let (_dir, conn, notes) = fixture();
    indexed(
        &conn,
        &notes,
        &[
            ("hamza.md", "رسالة إلى أحمد\n"),
            ("plain.md", "رساله الي احمد\n"),
        ],
    );

    assert_eq!(names(&conn, "احمد"), vec!["hamza.md", "plain.md"]);
    assert_eq!(names(&conn, "إلى أحمد"), vec!["hamza.md", "plain.md"]);
    assert_eq!(names(&conn, "رسالة"), vec!["hamza.md", "plain.md"]);
}

#[test]
fn a_cjk_word_is_found_inside_a_run() {
    let (_dir, conn, notes) = fixture();
    indexed(
        &conn,
        &notes,
        &[
            ("diary.md", "第一行\n我喜欢写作和读书\n"),
            ("other.md", "别的内容\n"),
        ],
    );

    let short = hits(&conn, "写作");
    assert_eq!(short.len(), 1);
    assert_eq!(short[0].title, "diary.md");
    assert_eq!(short[0].line, Some(2));
    assert_eq!(matched(&short[0]), vec!["写作"]);

    let long = hits(&conn, "喜欢写");
    assert_eq!(long.len(), 1);
    assert_eq!(long[0].line, Some(2));
    assert_eq!(matched(&long[0]), vec!["喜欢写"]);

    assert!(hits(&conn, "作文").is_empty());
}

#[test]
fn a_mixed_query_needs_every_run() {
    let (_dir, conn, notes) = fixture();
    indexed(
        &conn,
        &notes,
        &[
            ("plan.md", "Plan: 写作 every day\n"),
            ("words.md", "写作 only\n"),
        ],
    );

    assert_eq!(names(&conn, "写作 plan"), vec!["plan.md"]);
    assert_eq!(names(&conn, "写作"), vec!["plan.md", "words.md"]);
}

#[test]
fn a_latin_search_is_answered_by_the_word_index_as_before() {
    let (_dir, conn, notes) = fixture();
    indexed(&conn, &notes, &[("cafe.md", "Émile went to the café\n")]);

    assert_eq!(names(&conn, "emile"), vec!["cafe.md"]);
    assert_eq!(names(&conn, "cafe\u{301}"), vec!["cafe.md"]);
}

#[test]
fn a_forgotten_note_leaves_the_folded_index() {
    let (_dir, conn, notes) = fixture();
    indexed(&conn, &notes, &[("diary.md", "我喜欢写作\n")]);
    assert_eq!(names(&conn, "写作"), vec!["diary.md"]);

    std::fs::remove_file(notes.join("diary.md")).expect("remove");
    notes_index::reconcile(&conn, &notes, &|| false, &|_| false).expect("reconcile");

    assert!(hits(&conn, "写作").is_empty());
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM files_folded", [], |row| row.get(0))
        .expect("count");
    assert_eq!(rows, 0);
}

#[test]
fn only_notes_in_a_folded_script_take_a_folded_row() {
    let (_dir, conn, notes) = fixture();
    indexed(
        &conn,
        &notes,
        &[("latin.md", "plain text\n"), ("arabic.md", "نص\n")],
    );

    let rows: i64 = conn
        .query_row("SELECT count(*) FROM files_folded", [], |row| row.get(0))
        .expect("count");
    assert_eq!(rows, 1);
}

#[test]
fn migration_051_fills_the_folded_index_from_the_text_already_indexed() {
    let (_dir, conn, notes) = fixture();
    indexed(&conn, &notes, &[("diary.md", "我喜欢写作\n")]);
    conn.execute_batch("DROP TABLE files_folded; DELETE FROM schema_version WHERE version = 51;")
        .expect("back to 050");
    std::fs::remove_file(notes.join("diary.md")).expect("the file is not read again");

    run_migrations(&conn).expect("migrations");

    assert_eq!(names(&conn, "写作"), vec!["diary.md"]);
}

#[test]
fn a_lost_folded_index_is_recreated_and_refilled() {
    let (_dir, conn, notes) = fixture();
    indexed(&conn, &notes, &[("note.md", "مُلاحَظةٌ\n")]);
    conn.execute_batch("DROP TABLE files_folded;")
        .expect("drop");

    let outcome = repair_notes_index(&conn).expect("repair");

    assert_eq!(outcome, IndexRepairOutcome::Repaired);
    assert_eq!(names(&conn, "ملاحظة"), vec!["note.md"]);
}
