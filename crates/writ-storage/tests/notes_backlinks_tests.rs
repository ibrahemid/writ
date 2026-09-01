//! The backlink query: the notes that link to one note, and the sentence each
//! link sits in (spec L2, ADR-034).
//!
//! The sentence-cutting policy is tested in `writ_core::notes::snippet`. What
//! is tested here is which links the query returns and which it must not: an
//! ambiguous link belongs to every note it might mean, a link that resolved to
//! a different note of the same name belongs to that note alone, and a link
//! that resolved to nothing belongs to no list at all.

use std::path::Path;

use rusqlite::Connection;
use tempfile::TempDir;
use writ_storage::notes_index::{self, BacklinkCertainty, BacklinkRow, NotesIndex};

fn never_cancelled() -> impl Fn() -> bool {
    || false
}

fn never_dataless() -> impl Fn(&Path) -> bool {
    |_| false
}

fn fixture() -> (TempDir, Connection, std::path::PathBuf) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = writ_storage::database::connection::open_database(&db_path).expect("open_database");
    writ_storage::database::migrations::run_migrations(&conn).expect("migrations");
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("create notes dir");
    (dir, conn, notes)
}

fn write_note(notes: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = notes.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(&path, body).expect("write note");
    path
}

fn walk(conn: &Connection, notes: &Path) {
    notes_index::reconcile(conn, notes, &never_cancelled(), &never_dataless()).expect("reconcile");
}

fn backlinks(conn: &Connection, path: &Path) -> Vec<BacklinkRow> {
    NotesIndex::new(conn)
        .backlinks(&notes_index::index_key(path))
        .expect("backlinks")
}

/// The `from_name` of each row, which is what a list shows.
fn names(rows: &[BacklinkRow]) -> Vec<&str> {
    rows.iter().map(|row| row.from_name.as_str()).collect()
}

#[test]
fn lists_the_notes_that_link_here_with_the_sentence_each_link_is_in() {
    let (_dir, conn, notes) = fixture();
    let target = write_note(&notes, "Target.md", "# Target\n");
    write_note(
        &notes,
        "Source.md",
        "Some preamble. The plan is in [[Target]] as agreed. Another sentence.\n",
    );
    walk(&conn, &notes);

    let rows = backlinks(&conn, &target);
    assert_eq!(names(&rows), ["Source"]);
    assert_eq!(rows[0].to_target, "Target");
    assert_eq!(rows[0].kind, "wikilink");
    assert_eq!(rows[0].line, 1);
    assert_eq!(rows[0].certainty, BacklinkCertainty::Resolved);
    assert_eq!(rows[0].context, "The plan is in [[Target]] as agreed.");
}

#[test]
fn a_note_nothing_links_to_has_no_rows() {
    let (_dir, conn, notes) = fixture();
    let target = write_note(&notes, "Lonely.md", "# Lonely\n");
    write_note(&notes, "Other.md", "Nothing here.\n");
    walk(&conn, &notes);

    assert!(backlinks(&conn, &target).is_empty());
}

#[test]
fn finds_a_note_linked_by_name_from_two_folders() {
    let (_dir, conn, notes) = fixture();
    let target = write_note(&notes, "Target.md", "# Target\n");
    write_note(&notes, "one/First.md", "First says [[Target]].\n");
    write_note(&notes, "two/Second.md", "Second says [[Target]] too.\n");
    walk(&conn, &notes);

    let rows = backlinks(&conn, &target);
    assert_eq!(names(&rows), ["First", "Second"]);
    assert!(rows
        .iter()
        .all(|row| row.certainty == BacklinkCertainty::Resolved));
}

#[test]
fn carries_the_alias_a_link_is_displayed_as() {
    let (_dir, conn, notes) = fixture();
    let target = write_note(&notes, "Target.md", "# Target\n");
    write_note(&notes, "Source.md", "See [[Target|the plan]] for it.\n");
    walk(&conn, &notes);

    let rows = backlinks(&conn, &target);
    assert_eq!(rows[0].alias.as_deref(), Some("the plan"));
    assert_eq!(
        rows[0].to_target, "Target",
        "the target is what the link points at, never its label"
    );
}

#[test]
fn a_markdown_link_is_a_backlink_too() {
    let (_dir, conn, notes) = fixture();
    let target = write_note(&notes, "Target.md", "# Target\n");
    write_note(&notes, "Source.md", "See [the plan](Target.md) for it.\n");
    walk(&conn, &notes);

    let rows = backlinks(&conn, &target);
    assert_eq!(names(&rows), ["Source"]);
    assert_eq!(rows[0].kind, "markdown");
    assert_eq!(
        rows[0].alias, None,
        "a markdown label is not an alias; the sentence carries it"
    );
    assert_eq!(rows[0].context, "See [the plan](Target.md) for it.");
}

#[test]
fn an_ambiguous_link_is_listed_under_every_note_it_might_mean() {
    let (_dir, conn, notes) = fixture();
    let one = write_note(&notes, "projects/Meeting.md", "# Meeting\n");
    let two = write_note(&notes, "archive/Meeting.md", "# Meeting\n");
    write_note(&notes, "Diary.md", "Wrote up [[Meeting]] after.\n");
    walk(&conn, &notes);

    for target in [&one, &two] {
        let rows = backlinks(&conn, target);
        assert_eq!(
            names(&rows),
            ["Diary"],
            "an ambiguous link belongs to both lists, not to neither"
        );
        assert_eq!(rows[0].certainty, BacklinkCertainty::Ambiguous);
        assert_eq!(rows[0].context, "Wrote up [[Meeting]] after.");
    }
}

#[test]
fn a_link_that_reached_another_note_of_the_same_name_is_not_listed_here() {
    let (_dir, conn, notes) = fixture();
    // The shallower note wins the ranking, so the link resolves rather than
    // going ambiguous, and the deeper note is not what it reached.
    let shallow = write_note(&notes, "Meeting.md", "# Meeting\n");
    let deep = write_note(&notes, "archive/old/Meeting.md", "# Meeting\n");
    write_note(&notes, "Diary.md", "Wrote up [[Meeting]] after.\n");
    walk(&conn, &notes);

    assert_eq!(names(&backlinks(&conn, &shallow)), ["Diary"]);
    assert!(
        backlinks(&conn, &deep).is_empty(),
        "a link that resolved elsewhere belongs to the note it reached"
    );
}

#[test]
fn a_link_to_no_note_at_all_is_listed_under_no_note() {
    let (_dir, conn, notes) = fixture();
    let target = write_note(&notes, "Target.md", "# Target\n");
    write_note(&notes, "Source.md", "Points at [[Nowhere]] and nothing.\n");
    walk(&conn, &notes);

    assert!(backlinks(&conn, &target).is_empty());
    let unresolved: i64 = conn
        .query_row(
            "SELECT count(*) FROM links WHERE to_path IS NULL",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(unresolved, 1, "the link is still on record, just unreached");
}

#[test]
fn a_renamed_note_loses_the_links_that_named_its_old_name() {
    let (_dir, conn, notes) = fixture();
    let old = write_note(&notes, "Old.md", "# Old\n");
    write_note(&notes, "Source.md", "Refers to [[Old]] here.\n");
    walk(&conn, &notes);
    assert_eq!(names(&backlinks(&conn, &old)), ["Source"]);

    let new = notes.join("New.md");
    std::fs::rename(&old, &new).expect("rename");
    walk(&conn, &notes);

    assert!(
        backlinks(&conn, &new).is_empty(),
        "the link still names Old, so it is not a backlink of New"
    );
    let stale: i64 = conn
        .query_row(
            "SELECT count(*) FROM links WHERE to_path IS NULL",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(stale, 1, "the link is now unresolved, not still pointing");
}

#[test]
fn rewriting_the_link_puts_the_renamed_note_back_in_the_list() {
    let (_dir, conn, notes) = fixture();
    let old = write_note(&notes, "Old.md", "# Old\n");
    write_note(&notes, "Source.md", "Refers to [[Old]] here.\n");
    walk(&conn, &notes);

    let new = notes.join("New.md");
    std::fs::rename(&old, &new).expect("rename");
    write_note(&notes, "Source.md", "Refers to [[New]] here.\n");
    walk(&conn, &notes);

    let rows = backlinks(&conn, &new);
    assert_eq!(names(&rows), ["Source"]);
    assert_eq!(rows[0].context, "Refers to [[New]] here.");
}

#[test]
fn a_removed_linking_note_leaves_the_list() {
    let (_dir, conn, notes) = fixture();
    let target = write_note(&notes, "Target.md", "# Target\n");
    let source = write_note(&notes, "Source.md", "Refers to [[Target]] here.\n");
    write_note(&notes, "Keeper.md", "Also [[Target]] here.\n");
    walk(&conn, &notes);
    assert_eq!(names(&backlinks(&conn, &target)), ["Keeper", "Source"]);

    std::fs::remove_file(&source).expect("remove");
    walk(&conn, &notes);

    assert_eq!(names(&backlinks(&conn, &target)), ["Keeper"]);
}

#[test]
fn a_removed_note_has_no_backlinks_left_to_ask_for() {
    let (_dir, conn, notes) = fixture();
    let target = write_note(&notes, "Target.md", "# Target\n");
    write_note(&notes, "Source.md", "Refers to [[Target]] here.\n");
    walk(&conn, &notes);

    std::fs::remove_file(&target).expect("remove");
    walk(&conn, &notes);

    assert!(backlinks(&conn, &target).is_empty());
}

#[test]
fn a_note_the_index_holds_by_name_alone_is_listed_without_a_sentence() {
    let (_dir, conn, notes) = fixture();
    let target = write_note(&notes, "Target.md", "# Target\n");
    write_note(&notes, "Evicted.md", "Refers to [[Target]] here.\n");
    walk(&conn, &notes);
    assert_eq!(
        backlinks(&conn, &target)[0].context,
        "Refers to [[Target]] here."
    );

    // The file goes back to being a placeholder: its rows stay, its text does
    // not. The link is still on record, so the row is still in the list.
    conn.execute(
        "UPDATE files_fts SET content = '' WHERE rowid =
           (SELECT rowid FROM files WHERE path LIKE '%Evicted.md')",
        [],
    )
    .expect("empty the text");

    let rows = backlinks(&conn, &target);
    assert_eq!(names(&rows), ["Evicted"]);
    assert_eq!(rows[0].context, "", "no text to quote is no quote");
    assert_eq!(rows[0].line, 1, "the row still says where the link is");
}

#[test]
fn a_note_changed_outside_writ_updates_the_list_on_the_next_index_pass() {
    let (_dir, conn, notes) = fixture();
    let db_path = _dir.path().join("writ.db");
    let target = write_note(&notes, "Target.md", "# Target\n");
    let source = write_note(&notes, "Source.md", "Nothing here yet.\n");
    walk(&conn, &notes);
    assert!(backlinks(&conn, &target).is_empty());
    drop(conn);

    // What the watcher does with one changed file: index that path alone.
    std::fs::write(&source, "Now it says [[Target]] instead.\n").expect("rewrite");
    let store = notes_index::NotesIndexStore::open(&db_path).expect("open store");
    assert!(store.index_path(&source).expect("index one path"));

    let rows = store
        .backlinks(&notes_index::index_key(&target))
        .expect("backlinks");
    assert_eq!(names(&rows), ["Source"]);
    assert_eq!(rows[0].context, "Now it says [[Target]] instead.");
}

#[test]
fn emptying_the_link_table_and_walking_again_rebuilds_the_list() {
    let (_dir, conn, notes) = fixture();
    let target = write_note(&notes, "Target.md", "# Target\n");
    write_note(&notes, "Source.md", "Refers to [[Target]] here.\n");
    walk(&conn, &notes);
    let before = backlinks(&conn, &target);
    assert_eq!(before.len(), 1);

    conn.execute("DELETE FROM links", []).expect("drop links");
    assert!(backlinks(&conn, &target).is_empty());

    walk(&conn, &notes);
    assert_eq!(
        backlinks(&conn, &target),
        before,
        "the list is derived from the files, so a walk brings it back whole"
    );
}

#[test]
fn orders_the_list_by_note_then_by_where_the_link_is() {
    let (_dir, conn, notes) = fixture();
    let target = write_note(&notes, "Target.md", "# Target\n");
    write_note(
        &notes,
        "B.md",
        "First [[Target]] mention.\n\nSecond [[Target]] mention.\n",
    );
    write_note(&notes, "A.md", "Only [[Target]] mention.\n");
    walk(&conn, &notes);

    let rows = backlinks(&conn, &target);
    assert_eq!(names(&rows), ["A", "B", "B"]);
    assert_eq!(rows[1].line, 1);
    assert_eq!(rows[2].line, 3);
}

#[test]
fn a_note_linking_to_itself_is_in_its_own_list() {
    let (_dir, conn, notes) = fixture();
    let target = write_note(&notes, "Target.md", "# Target\n\nSee [[Target]] again.\n");
    walk(&conn, &notes);

    let rows = backlinks(&conn, &target);
    assert_eq!(names(&rows), ["Target"]);
    assert_eq!(rows[0].from_path, notes_index::index_key(&target));
}
