use std::collections::HashSet;
use std::path::Path;

use rusqlite::Connection;
use tempfile::TempDir;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::notes_index::{self, IndexedBy, IndexedNote, NotesIndex, NotesIndexStore};

fn never_cancelled() -> impl Fn() -> bool {
    || false
}

fn never_dataless() -> impl Fn(&Path) -> bool {
    |_| false
}

fn fixture() -> (TempDir, Connection, std::path::PathBuf) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
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

fn search(conn: &Connection, raw: &str) -> Vec<String> {
    let index = NotesIndex::new(conn);
    let query = writ_core::search::to_prefix_match(raw).expect("query");
    let terms = writ_core::search::search_terms(raw);
    index
        .search_hits(&query, &terms, 50)
        .expect("search_hits")
        .into_iter()
        .map(|hit| hit.path.expect("a notes-index hit always carries its path"))
        .collect()
}

fn search_names(conn: &Connection, notes: &Path, raw: &str) -> Vec<String> {
    NotesIndex::new(conn)
        .search_names(raw, notes, 50)
        .expect("search_names")
        .into_iter()
        .map(|hit| hit.path)
        .collect()
}

/// Takes every read permission off `path`, returning `false` only on a
/// platform where permission bits do not stop a read.
///
/// On unix it must work, and it panics when it does not rather than returning
/// `false`: root ignores the bits, and a caller that quietly took the `false`
/// branch there would report a green test that asserted nothing.
#[cfg(unix)]
fn deny_reads(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000))
        .expect("set permissions");
    assert!(
        std::fs::read_to_string(path).is_err(),
        "a file with mode 000 was still readable, so this test proves \
         nothing about reads; run the suite as a normal user"
    );
    true
}

#[cfg(not(unix))]
fn deny_reads(path: &Path) -> bool {
    let _ = path;
    false
}

fn indexed_paths(conn: &Connection) -> HashSet<String> {
    NotesIndex::new(conn)
        .snapshot()
        .expect("snapshot")
        .into_iter()
        .map(|(path, _, _)| path)
        .collect()
}

#[test]
fn a_note_created_outside_writ_is_findable_by_its_text_after_reconcile() {
    let (_dir, conn, notes) = fixture();
    let path = write_note(&notes, "field-notes.md", "the peregrine stoops at dawn");

    let outcome = notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("reconcile");
    assert_eq!(outcome.added, 1, "the new file must be added");
    assert!(!outcome.cancelled);

    assert_eq!(
        search(&conn, "peregrine"),
        vec![notes_index::index_key(&path)]
    );
}

#[test]
fn a_note_deleted_outside_writ_disappears_from_results() {
    let (_dir, conn, notes) = fixture();
    let path = write_note(&notes, "gone.md", "ephemeral marker text");
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless()).expect("first");
    assert_eq!(search(&conn, "ephemeral").len(), 1);

    std::fs::remove_file(&path).expect("remove");
    let outcome = notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("second");

    assert_eq!(outcome.removed, 1, "the vanished file must be removed");
    assert!(search(&conn, "ephemeral").is_empty());
}

#[test]
fn deleting_the_database_and_relaunching_rebuilds_the_index_and_returns_the_same_results() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("create notes dir");
    write_note(&notes, "alpha.md", "harbour seals bask on the shingle");
    write_note(&notes, "beta.md", "the shingle is grey and wet");

    let first = {
        let conn = open_database(&db_path).expect("open");
        run_migrations(&conn).expect("migrations");
        notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
            .expect("reconcile");
        search(&conn, "shingle")
    };
    assert_eq!(first.len(), 2);

    std::fs::remove_file(&db_path).expect("delete database");

    let second = {
        let conn = open_database(&db_path).expect("reopen");
        run_migrations(&conn).expect("migrations");
        let outcome = notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
            .expect("rebuild");
        assert_eq!(outcome.added, 2, "an empty index rebuilds every file");
        search(&conn, "shingle")
    };

    assert_eq!(first, second, "a rebuilt index returns the same results");
}

#[test]
fn reconcile_indexes_a_file_reported_as_not_downloaded_by_name_only() {
    let (_dir, conn, notes) = fixture();
    let downloaded = write_note(&notes, "here.md", "sundial marker");
    let dataless = write_note(&notes, "cloud.md", "sundial marker");

    let dataless_key = notes_index::index_key(&dataless);
    let stub = {
        let target = dataless.clone();
        move |candidate: &Path| candidate == target.as_path()
    };

    let outcome =
        notes_index::reconcile(&conn, &notes, &never_cancelled(), &stub).expect("reconcile");

    assert_eq!(outcome.skipped_dataless, 1);
    assert_eq!(outcome.added, 2);

    let indexed = indexed_paths(&conn);
    assert!(indexed.contains(&notes_index::index_key(&downloaded)));
    assert!(
        indexed.contains(&dataless_key),
        "a file reported as not downloaded is still one of the notes"
    );
    assert_eq!(
        search(&conn, "sundial"),
        vec![notes_index::index_key(&downloaded)],
        "only the downloaded file's content was read"
    );
    assert_eq!(
        search_names(&conn, &notes, "cloud"),
        vec![dataless_key.clone()],
        "the name is what an undownloaded note is findable by"
    );

    let second =
        notes_index::reconcile(&conn, &notes, &never_cancelled(), &stub).expect("second pass");
    assert_eq!(
        second.removed, 0,
        "the name-only row survives a second pass"
    );
    assert_eq!(second.added, 0);
    assert!(indexed_paths(&conn).contains(&dataless_key));
}

#[test]
fn a_file_downloaded_outside_writ_gets_its_text_into_the_index() {
    // Materialising a placeholder leaves the size and the mtime where they
    // were: that is what makes it a placeholder. The size and mtime shortcut
    // would therefore keep the name-only row for good, and the note would stay
    // unsearchable by its text forever.
    let (_dir, conn, notes) = fixture();
    let note = write_note(&notes, "cloud.md", "sundial marker");
    let key = notes_index::index_key(&note);

    let stub = {
        let target = note.clone();
        move |candidate: &Path| candidate == target.as_path()
    };
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &stub).expect("first");
    assert!(search(&conn, "sundial").is_empty());

    // The file is not touched: only the provider's answer changes.
    let outcome = notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("second");

    assert_eq!(outcome.updated, 1);
    assert_eq!(
        search(&conn, "sundial"),
        vec![key.clone()],
        "the text of a downloaded note joins the index"
    );

    let indexed_by: String = conn
        .query_row(
            "SELECT indexed_by FROM files WHERE path = ?1",
            [&key],
            |r| r.get(0),
        )
        .expect("indexed_by");
    assert_eq!(indexed_by, "content", "the row holds text now, and says so");
}

#[test]
fn a_digest_written_over_a_name_only_row_does_not_cost_it_its_text() {
    // A later writer recording a content hash is the obvious thing to do with
    // `files.hash`, and it used to be where the name-only mark lived. Recovery
    // reads `indexed_by`, so a digest landing on the row changes nothing about
    // it.
    let (_dir, conn, notes) = fixture();
    let note = write_note(&notes, "cloud.md", "sundial marker");
    let key = notes_index::index_key(&note);

    let stub = {
        let target = note.clone();
        move |candidate: &Path| candidate == target.as_path()
    };
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &stub).expect("first");

    conn.execute(
        "UPDATE files SET hash = ?2 WHERE path = ?1",
        rusqlite::params![
            key,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        ],
    )
    .expect("a later writer records a digest");

    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless()).expect("second");

    assert_eq!(
        search(&conn, "sundial"),
        vec![key],
        "the download is still recognised with a digest on the row"
    );
}

#[test]
fn reconcile_never_reads_a_file_reported_as_not_downloaded() {
    // Reading a placeholder is what asks the provider to materialise it, so
    // the walk has to decide from metadata alone. The file is made unreadable
    // first: a walk that opened it would fail the read and drop the file, so
    // the row proves the read never happened.
    let (_dir, conn, notes) = fixture();
    let dataless = write_note(&notes, "evicted.md", "sundial marker");
    if !deny_reads(&dataless) {
        return;
    }

    let stub = {
        let target = dataless.clone();
        move |candidate: &Path| candidate == target.as_path()
    };
    let outcome =
        notes_index::reconcile(&conn, &notes, &never_cancelled(), &stub).expect("reconcile");

    assert_eq!(outcome.added, 1);
    assert!(indexed_paths(&conn).contains(&notes_index::index_key(&dataless)));
    assert!(
        search(&conn, "sundial").is_empty(),
        "no text can have been read out of an unreadable file"
    );
}

#[test]
fn reconcile_leaves_out_the_files_a_sync_client_or_editor_left_behind() {
    let (_dir, conn, notes) = fixture();
    let note = write_note(&notes, "note.md", "kestrel marker");
    write_note(&notes, ".syncthing.note.md.tmp", "kestrel marker");
    write_note(&notes, ".note.md.icloud", "kestrel marker");
    write_note(&notes, ".note.md.swp", "kestrel marker");
    write_note(&notes, "note.md~", "kestrel marker");
    let copy = write_note(
        &notes,
        "note.sync-conflict-20260822-120000-ABCD.md",
        "kestrel marker",
    );

    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("reconcile");

    let mut indexed: Vec<String> = indexed_paths(&conn).into_iter().collect();
    indexed.sort();
    let mut expected = vec![notes_index::index_key(&note), notes_index::index_key(&copy)];
    expected.sort();
    assert_eq!(
        indexed, expected,
        "only the note and the copy that holds somebody's text are indexed"
    );
}

#[test]
fn reconcile_leaves_out_a_file_inside_a_sync_clients_own_folder() {
    let (_dir, conn, notes) = fixture();
    let note = write_note(&notes, "note.md", "kestrel marker");
    write_note(&notes, ".dropbox.cache/stale.md", "kestrel marker");

    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("reconcile");

    assert_eq!(
        indexed_paths(&conn).into_iter().collect::<Vec<_>>(),
        vec![notes_index::index_key(&note)]
    );
}

#[test]
fn reconcile_is_idempotent() {
    let (_dir, conn, notes) = fixture();
    write_note(&notes, "one.md", "first body");
    write_note(&notes, "two.md", "second body");

    let first = notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("first");
    assert_eq!(first.added, 2);

    let second = notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("second");
    assert_eq!(second.added, 0);
    assert_eq!(second.updated, 0);
    assert_eq!(second.removed, 0);
    assert_eq!(indexed_paths(&conn).len(), 2);
}

#[test]
fn reconcile_removes_rows_for_vanished_files() {
    let (_dir, conn, notes) = fixture();
    write_note(&notes, "stays.md", "kept");
    let doomed = write_note(&notes, "leaves.md", "removed");
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless()).expect("first");

    std::fs::remove_file(&doomed).expect("remove");
    let outcome = notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("second");

    assert_eq!(outcome.removed, 1);
    assert_eq!(indexed_paths(&conn).len(), 1);
}

#[test]
fn reconcile_updates_a_row_whose_mtime_and_size_changed() {
    let (_dir, conn, notes) = fixture();
    let path = write_note(&notes, "edited.md", "before");
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless()).expect("first");
    assert!(search(&conn, "before").len() == 1);

    // Size and mtime are what the walk compares against; rewriting with a
    // longer body changes both, and size alone differs even on a filesystem
    // whose mtime granularity swallows the second write.
    std::fs::write(&path, "after the rewrite, longer than before").expect("rewrite");

    let outcome = notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("second");

    assert_eq!(outcome.updated, 1);
    assert_eq!(outcome.added, 0);
    assert_eq!(search(&conn, "rewrite").len(), 1);
}

#[test]
fn search_names_ranks_a_prefix_match_first() {
    let (_dir, conn, notes) = fixture();
    write_note(&notes, "meeting-notes.md", "body");
    write_note(&notes, "the-meeting.md", "body");
    write_note(&notes, "unrelated.md", "body");
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("reconcile");

    let hits = NotesIndex::new(&conn)
        .search_names("meeting", &notes, 10)
        .expect("search_names");

    assert!(hits.len() >= 2, "both meeting notes must match");
    assert_eq!(
        hits[0].name, "meeting-notes.md",
        "a prefix match outranks a mid-name match"
    );
    assert!(hits.iter().all(|hit| hit.name != "unrelated.md"));
}

#[test]
fn search_names_ranks_the_path_inside_the_notes_folder_only() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
    // A folder above the root spelling out the query, the way a home directory
    // named `claude` spells out "cloud".
    let notes = dir.path().join("c-l-o-u-d").join("notes");
    std::fs::create_dir_all(&notes).expect("create notes dir");
    let plain = write_note(&notes, "here.md", "body");
    let nested = write_note(&notes, "projects/alpha.md", "body");
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("reconcile");

    assert!(
        notes_index::index_key(&plain).contains("c-l-o-u-d"),
        "the row is keyed by a path that spells the query out"
    );
    assert!(
        search_names(&conn, &notes, "cloud").is_empty(),
        "the folders above the notes folder are no part of a note's name"
    );
    assert_eq!(
        search_names(&conn, &notes, "projalpha"),
        vec![notes_index::index_key(&nested)],
        "a folder inside the notes folder is"
    );
}

#[test]
fn obsidian_folder_contents_are_not_indexed() {
    let (_dir, conn, notes) = fixture();
    write_note(&notes, "kept.md", "workspace marker");
    write_note(&notes, ".obsidian/plugins/whatever.md", "workspace marker");

    let outcome = notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("reconcile");

    assert_eq!(
        outcome.added, 1,
        "only the note outside .obsidian is indexed"
    );
    assert_eq!(search(&conn, "workspace").len(), 1);
}

#[test]
fn upsert_preserves_the_rowid_and_the_rows_that_cascade_from_it() {
    let (_dir, conn, notes) = fixture();
    let path = write_note(&notes, "linked.md", "first body");
    let key = notes_index::index_key(&path);
    let index = NotesIndex::new(&conn);

    let note = IndexedNote {
        path: key.clone(),
        name: "linked.md".to_string(),
        size: 10,
        mtime: 1,
        hash: None,
        indexed_by: IndexedBy::Content,
    };
    index.upsert(&note, "first body").expect("first upsert");

    let rowid_before: i64 = conn
        .query_row("SELECT rowid FROM files WHERE path = ?1", [&key], |row| {
            row.get(0)
        })
        .expect("rowid");
    conn.execute(
        "INSERT INTO links (from_path, to_target, to_path, kind, line, col)
         VALUES (?1, 'elsewhere', NULL, 'wiki', 1, 0)",
        [&key],
    )
    .expect("insert link");

    let updated = IndexedNote {
        size: 20,
        mtime: 2,
        ..note
    };
    index
        .upsert(&updated, "second body")
        .expect("second upsert");

    let rowid_after: i64 = conn
        .query_row("SELECT rowid FROM files WHERE path = ?1", [&key], |row| {
            row.get(0)
        })
        .expect("rowid");
    assert_eq!(
        rowid_before, rowid_after,
        "an upsert must not reassign the rowid files_fts joins on"
    );

    let links: i64 = conn
        .query_row(
            "SELECT count(*) FROM links WHERE from_path = ?1",
            [&key],
            |row| row.get(0),
        )
        .expect("count links");
    assert_eq!(links, 1, "an upsert must not cascade the derived rows away");

    assert_eq!(search(&conn, "second"), vec![key.clone()]);
    assert!(
        search(&conn, "first").is_empty(),
        "the replaced text leaves the index"
    );
}

#[test]
fn remove_takes_the_rows_that_cascade_from_the_file_with_it() {
    let (_dir, conn, notes) = fixture();
    let path = write_note(&notes, "doomed.md", "body text");
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("reconcile");
    let key = notes_index::index_key(&path);

    conn.execute(
        "INSERT INTO tags (path, tag, line) VALUES (?1, 'reading', 3)",
        [&key],
    )
    .expect("insert tag");

    NotesIndex::new(&conn).remove(&key).expect("remove");

    let tags: i64 = conn
        .query_row("SELECT count(*) FROM tags WHERE path = ?1", [&key], |row| {
            row.get(0)
        })
        .expect("count tags");
    assert_eq!(tags, 0, "removing a file removes its derived rows");
    assert!(search(&conn, "body").is_empty());
}

#[test]
fn the_index_key_of_a_walked_path_matches_the_key_of_a_watcher_path() {
    let (_dir, conn, notes) = fixture();
    let path = write_note(&notes, "shared.md", "one key policy");
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("reconcile");

    // The watcher hands back an event path built from the root it was given,
    // which on macOS is the pre-canonical spelling (`/var` for `/private/var`).
    let watcher_path = notes.join("shared.md");
    let walked = indexed_paths(&conn);

    assert!(
        walked.contains(&notes_index::index_key(&watcher_path)),
        "the walk and the watcher must produce one key for one file"
    );
    assert_eq!(
        notes_index::index_key(&path),
        notes_index::index_key(&watcher_path)
    );
}

#[test]
fn reconcile_stops_when_cancelled() {
    let (_dir, conn, notes) = fixture();
    for idx in 0..50 {
        write_note(&notes, &format!("note-{idx:03}.md"), "cancellable body");
    }

    let outcome =
        notes_index::reconcile(&conn, &notes, &|| true, &never_dataless()).expect("reconcile");

    assert!(outcome.cancelled, "a cancelled reconcile reports it");
    assert!(
        outcome.added < 50,
        "a cancelled reconcile stops before the whole tree"
    );
}

// Query-behaviour assertions ported from the retired `fts_tests.rs`. The
// tokenizer, the prefix index and the snippet builder are unchanged by the
// re-key to paths (migration 040 recreates them verbatim), and these are what
// prove it.

#[test]
fn a_hit_carries_the_matching_line_number_and_a_highlighted_snippet() {
    let (_dir, conn, notes) = fixture();
    write_note(
        &notes,
        "notes.md",
        "first line\nthe rerank ceiling here\ntail",
    );
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("reconcile");

    let hits = NotesIndex::new(&conn)
        .search_hits("\"rerank\"*", &["rerank".to_string()], 50)
        .expect("search_hits");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title, "notes.md");
    assert_eq!(hits[0].line, Some(2));
    let snippet: String = hits[0].snippet.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(snippet, "the rerank ceiling here");
    assert!(hits[0]
        .snippet
        .iter()
        .any(|s| s.matched && s.text == "rerank"));
}

#[test]
fn count_reports_total_matches_independent_of_limit() {
    let (_dir, conn, notes) = fixture();
    for idx in 0..5 {
        write_note(&notes, &format!("doc-{idx}.md"), "shared keyword body");
    }
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("reconcile");

    let index = NotesIndex::new(&conn);
    let hits = index
        .search_hits("\"keyword\"*", &["keyword".to_string()], 2)
        .expect("search_hits");

    assert_eq!(hits.len(), 2, "limit caps returned hits");
    assert_eq!(index.count("\"keyword\"*").expect("count"), 5);
}

#[test]
fn a_prefix_query_matches_longer_tokens() {
    // The prefix index (migration 030, recreated by 040) is what makes
    // search-as-you-type work: a 3-character prefix term must hit longer
    // tokens that share the prefix.
    let (_dir, conn, notes) = fixture();
    let hit = write_note(&notes, "tokenizer.md", "the token stream is tokenized");
    write_note(&notes, "other.md", "nothing in common");
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("reconcile");

    assert_eq!(search(&conn, "tok"), vec![notes_index::index_key(&hit)]);
}

#[test]
fn diacritics_are_folded_for_search() {
    // The unicode61 remove_diacritics=2 tokenizer folds accents, so an ASCII
    // query finds an accented term and vice versa.
    let (_dir, conn, notes) = fixture();
    let path = write_note(&notes, "cafe.md", "résumé of the meeting");
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("reconcile");

    let key = notes_index::index_key(&path);
    assert_eq!(search(&conn, "resume"), vec![key.clone()]);
    assert_eq!(search(&conn, "résumé"), vec![key]);
}

#[test]
fn migrations_are_idempotent_after_the_buffer_index_is_dropped() {
    // Migration 041 drops a table. Re-running migrations on an already
    // migrated database must be a clean no-op, never failing on a table that
    // is no longer there.
    let (_dir, conn, notes) = fixture();
    run_migrations(&conn).expect("second migration run must be a no-op");
    run_migrations(&conn).expect("third migration run must be a no-op");

    write_note(&notes, "still-works.md", "token streams");
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("reconcile");
    assert_eq!(search(&conn, "tok").len(), 1);
}

#[test]
fn the_buffer_index_is_gone_after_migration() {
    let (_dir, conn, _notes) = fixture();
    let remaining: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name = 'buffer_fts'",
            [],
            |row| row.get(0),
        )
        .expect("query sqlite_master");
    assert_eq!(remaining, 0, "migration 041 drops buffer_fts");
}

#[test]
fn a_note_over_the_large_file_ceiling_is_left_out_of_the_index() {
    let (dir, conn, notes) = fixture();
    // The save path skips buffers over THRESHOLD_NORMAL_BYTES, so a walk that
    // indexed them would hold their first contents for good: nothing would
    // ever update the row.
    let ceiling = writ_core::file_ops::THRESHOLD_NORMAL_BYTES as usize;
    let body = format!("gargantuan{}", "x".repeat(ceiling));
    write_note(&notes, "huge.md", &body);
    write_note(&notes, "small.md", "gargantuan");

    let outcome =
        notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless()).expect("walk");

    assert_eq!(outcome.added, 1);
    assert_eq!(search(&conn, "gargantuan").len(), 1);

    // The watcher arm answers the same way, or a later write would put back
    // what the walk left out.
    let store = NotesIndexStore::open(&dir.path().join("writ.db")).expect("open store");
    assert!(!store
        .index_path(&notes.join("huge.md"))
        .expect("index huge"));
    assert!(store
        .index_path(&notes.join("small.md"))
        .expect("index small"));
}

#[test]
fn rekey_root_moves_rows_and_keeps_the_rows_that_cascade_from_them() {
    let (dir, conn, notes) = fixture();
    let path = write_note(&notes, "moved.md", "portable text");
    let sibling = dir.path().join("Writing");
    std::fs::create_dir_all(&sibling).expect("sibling folder");
    let outsider = write_note(&sibling, "stays.md", "portable text");

    let index = NotesIndex::new(&conn);
    for file in [&path, &outsider] {
        let note = IndexedNote::from_file(file).expect("note");
        index.upsert(&note, "portable text").expect("upsert");
    }
    let key = notes_index::index_key(&path);
    let outside_key = notes_index::index_key(&outsider);
    conn.execute(
        "INSERT INTO links (from_path, to_target, to_path, kind, line, col)
         VALUES (?1, 'elsewhere', ?1, 'wiki', 1, 0)",
        [&key],
    )
    .expect("insert link");
    conn.execute(
        "INSERT INTO tags (path, tag, line) VALUES (?1, 'draft', 0)",
        [&key],
    )
    .expect("insert tag");

    let rowid_before: i64 = conn
        .query_row("SELECT rowid FROM files WHERE path = ?1", [&key], |row| {
            row.get(0)
        })
        .expect("rowid");

    let destination = dir.path().join("Moved");
    std::fs::rename(&notes, &destination).expect("move the folder");
    let to = std::fs::canonicalize(&destination).expect("canonical destination");

    let changed = NotesIndex::new(&conn)
        .rekey_root(&notes, &to)
        .expect("rekey");

    assert_eq!(
        changed, 1,
        "only the rows under the old folder are re-keyed"
    );
    let moved_key = notes_index::index_key(&destination.join("moved.md"));
    assert_eq!(
        search(&conn, "portable"),
        vec![moved_key.clone(), outside_key]
    );

    let rowid_after: i64 = conn
        .query_row(
            "SELECT rowid FROM files WHERE path = ?1",
            [&moved_key],
            |row| row.get(0),
        )
        .expect("rowid");
    assert_eq!(
        rowid_before, rowid_after,
        "re-keying must not reassign the rowid files_fts joins on"
    );

    let links: i64 = conn
        .query_row(
            "SELECT count(*) FROM links WHERE from_path = ?1 AND to_path = ?1",
            [&moved_key],
            |row| row.get(0),
        )
        .expect("count links");
    assert_eq!(links, 1, "both link columns follow the file");
    let tags: i64 = conn
        .query_row(
            "SELECT count(*) FROM tags WHERE path = ?1",
            [&moved_key],
            |row| row.get(0),
        )
        .expect("count tags");
    assert_eq!(tags, 1, "the cascading rows follow the file");
}

#[test]
fn a_reconcile_after_a_rekey_reads_nothing() {
    let (dir, conn, notes) = fixture();
    write_note(&notes, "one.md", "first");
    write_note(&notes, "two.md", "second");
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless()).expect("walk");

    let destination = dir.path().join("Elsewhere");
    std::fs::rename(&notes, &destination).expect("move the folder");
    let to = std::fs::canonicalize(&destination).expect("canonical destination");
    NotesIndex::new(&conn)
        .rekey_root(&notes, &to)
        .expect("rekey");

    let outcome =
        notes_index::reconcile(&conn, &to, &never_cancelled(), &never_dataless()).expect("walk");

    // Size and mtime survive the move, so the walk finds nothing to do. A
    // rekey that dropped the rows instead would make every note look new, and
    // in a sync folder reading every note pulls down every note.
    assert_eq!((outcome.added, outcome.updated, outcome.removed), (0, 0, 0));
}

#[test]
fn a_walk_gives_up_when_the_notes_folder_moves_under_it() {
    let (dir, _conn, notes) = fixture();
    write_note(&notes, "one.md", "first");
    let store = NotesIndexStore::open(&dir.path().join("writ.db")).expect("open store");

    let mine = store.generation();
    store.bump_generation();
    let cancelled = || store.generation() != mine;

    let outcome = store
        .reconcile(&notes, &cancelled, &notes_index::is_dataless)
        .expect("walk");

    assert!(outcome.cancelled, "a retired walk stops before it prunes");
    assert_eq!(outcome.added, 0);
}

#[test]
fn a_walk_does_not_overwrite_a_row_that_changed_while_it_was_reading() {
    let (_dir, conn, notes) = fixture();
    let path = write_note(&notes, "race.md", "first");
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless()).expect("first");

    std::fs::write(&path, "second body, longer than the first").expect("rewrite");
    let key = notes_index::index_key(&path);
    let current = IndexedNote::from_file(&path).expect("note");

    // The walk asks this of every file before it reads it, which is the window
    // a save lands in: the row is brought up to date here, after the walk took
    // its snapshot and before it writes.
    let saved_first_instead = |candidate: &Path| {
        if candidate == path {
            NotesIndex::new(&conn)
                .upsert(&current, "the save that got there first")
                .expect("competing write");
        }
        false
    };

    let outcome = notes_index::reconcile(&conn, &notes, &never_cancelled(), &saved_first_instead)
        .expect("second");

    assert_eq!(
        outcome.updated, 0,
        "the walk declines a row it no longer recognises"
    );
    assert_eq!(search(&conn, "first"), vec![key], "the newer text stands");
    assert!(
        search(&conn, "longer").is_empty(),
        "what the walk read is not written over it"
    );
}

#[test]
fn the_file_an_atomic_save_writes_before_the_rename_is_not_indexed() {
    let (_dir, conn, notes) = fixture();
    write_note(&notes, ".tmpAbCdEf", "half a note");
    write_note(&notes, "whole.tmp", "half a note");
    write_note(&notes, "whole.md", "half a note");

    let outcome =
        notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless()).expect("walk");

    assert_eq!(outcome.added, 1);
    assert_eq!(
        search(&conn, "half"),
        vec![notes_index::index_key(&notes.join("whole.md"))]
    );
}

#[test]
fn a_note_that_stops_being_indexable_loses_its_row() {
    let (_dir, conn, notes) = fixture();
    let binary = write_note(&notes, "turned-binary.md", "searchable words");
    let grown = write_note(&notes, "grown.md", "searchable words");
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless()).expect("first");
    assert_eq!(search(&conn, "searchable").len(), 2);

    std::fs::write(&binary, [0xff, 0xfe, 0x00, 0x01]).expect("no longer text");
    let ceiling = writ_core::file_ops::THRESHOLD_NORMAL_BYTES as usize;
    std::fs::write(&grown, "x".repeat(ceiling + 1)).expect("past the ceiling");

    let outcome = notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless())
        .expect("second");

    // A row kept for either would answer searches with text the file no longer
    // holds, and open a file that does not contain the words that found it.
    assert_eq!(outcome.removed, 2);
    assert!(search(&conn, "searchable").is_empty());
}
