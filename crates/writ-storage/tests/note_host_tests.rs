//! The capability check, and what every method answers once it passes.
//!
//! The check is the first line of each method, so the refusal tests here are
//! about what did *not* happen: a refused write leaves the file's bytes and its
//! modification time alone and writes no conflict copy, a refused read of a
//! path outside the folder says the caller may not read rather than where the
//! path went, and a refused index read on a host with no index says the same
//! rather than that the index is missing. Each of those three would be a
//! different answer if the check ran second.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use tempfile::TempDir;
use writ_core::config::FileExtension;
use writ_core::notes::containment::resolve_for_containment;
use writ_core::notes::host::{Capability, HostError, LastKnown, NoteHost, PermissionSet};
use writ_core::notes::WriteOrigin;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::note_host::NoteHostImpl;
use writ_storage::notes_index;

/// A notes folder and the path an index would live at.
struct Fixture {
    _dir: TempDir,
    notes: PathBuf,
    db: PathBuf,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().expect("temp dir");
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("notes folder");
    Fixture {
        db: dir.path().join("writ.db"),
        notes,
        _dir: dir,
    }
}

fn write_note(fixture: &Fixture, name: &str, body: &str) -> PathBuf {
    let file = fixture.notes.join(name);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).expect("note folder");
    }
    std::fs::write(&file, body).expect("seed a note");
    file
}

/// Walks the folder into a fresh index, so the index-backed methods have rows.
fn build_index(fixture: &Fixture) {
    let conn = open_database(&fixture.db).expect("open the database");
    run_migrations(&conn).expect("migrations");
    notes_index::reconcile(&conn, &fixture.notes, &|| false, &|_| false).expect("walk the folder");
}

fn held(capabilities: &[Capability]) -> PermissionSet {
    capabilities.iter().copied().collect()
}

/// A host over the folder alone, holding `capabilities`.
fn host(fixture: &Fixture, capabilities: &[Capability]) -> NoteHostImpl<'static> {
    NoteHostImpl::open(&fixture.notes, None, held(capabilities), FileExtension::Md)
        .expect("open the host")
}

/// A host over the folder and the index it was walked into.
fn indexed_host(fixture: &Fixture, capabilities: &[Capability]) -> NoteHostImpl<'static> {
    NoteHostImpl::open(
        &fixture.notes,
        Some(&fixture.db),
        held(capabilities),
        FileExtension::Md,
    )
    .expect("open the host")
}

fn origin() -> WriteOrigin {
    WriteOrigin::Mcp {
        client: "a program".to_string(),
    }
}

fn refused(error: HostError, capability: Capability) {
    assert_eq!(error, HostError::NotPermitted { capability });
}

fn modified(file: &Path) -> Option<SystemTime> {
    std::fs::metadata(file).expect("metadata").modified().ok()
}

/// Every entry under the notes folder, at any depth, sorted.
fn folder_contents(notes: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut queue = vec![notes.to_path_buf()];
    while let Some(dir) = queue.pop() {
        for entry in std::fs::read_dir(&dir).expect("read the notes folder") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                queue.push(path);
                continue;
            }
            found.push(
                path.file_name()
                    .expect("a name")
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    found.sort();
    found
}

#[test]
fn a_host_holding_nothing_refuses_every_method() {
    let fixture = fixture();
    write_note(&fixture, "Launch.md", "before\n");
    build_index(&fixture);
    let host = indexed_host(&fixture, &[]);

    refused(
        host.list_notes(None, 10).expect_err("list"),
        Capability::ListNotes,
    );
    refused(
        host.read_note("Launch.md").expect_err("read"),
        Capability::ReadNote,
    );
    refused(
        host.note_summary("Launch.md").expect_err("summary"),
        Capability::ReadNote,
    );
    refused(
        host.search_notes("before", 10).expect_err("search"),
        Capability::SearchNotes,
    );
    refused(
        host.note_links("Launch.md").expect_err("links"),
        Capability::ReadIndex,
    );
    refused(
        host.note_backlinks("Launch.md").expect_err("backlinks"),
        Capability::ReadIndex,
    );
    refused(
        host.note_facts("Launch.md").expect_err("facts"),
        Capability::ReadIndex,
    );
    refused(host.folder_tags().expect_err("tags"), Capability::ReadIndex);
    refused(
        host.write_note("Launch.md", "after\n", LastKnown::Overwrite, origin())
            .expect_err("write"),
        Capability::WriteNote,
    );
    refused(
        host.create_note("Ship it", "text\n", origin())
            .expect_err("create"),
        Capability::CreateNote,
    );
    refused(
        host.rename_note("Launch.md", "Landed", origin())
            .expect_err("rename"),
        Capability::RenameNote,
    );
}

#[test]
fn a_refused_write_leaves_the_file_as_it_was_and_writes_nothing_beside_it() {
    let fixture = fixture();
    let note = write_note(&fixture, "Launch.md", "before\n");
    let before = modified(&note);
    let host = host(&fixture, &[Capability::ReadNote]);

    refused(
        host.write_note("Launch.md", "after\n", LastKnown::Overwrite, origin())
            .expect_err("a read-only set has no write path"),
        Capability::WriteNote,
    );

    assert_eq!(
        std::fs::read_to_string(&note).expect("read back"),
        "before\n"
    );
    assert_eq!(modified(&note), before, "the file was not touched");
    assert_eq!(
        folder_contents(&fixture.notes),
        vec!["Launch.md".to_string()],
        "a refusal writes no conflict copy: there was no write to refuse"
    );
}

#[test]
fn a_refused_read_answers_before_it_resolves_the_path() {
    let fixture = fixture();
    let outside = fixture.notes.parent().expect("a parent").join("outside.md");
    std::fs::write(&outside, "not a note of this folder\n").expect("seed");
    let host = host(&fixture, &[Capability::ListNotes]);

    refused(
        host.read_note(&outside.to_string_lossy())
            .expect_err("a path out of the folder"),
        Capability::ReadNote,
    );
    refused(
        host.read_note("Missing.md")
            .expect_err("a path with no file"),
        Capability::ReadNote,
    );
}

#[test]
fn a_refused_index_read_answers_before_it_asks_for_the_index() {
    let fixture = fixture();
    write_note(&fixture, "Launch.md", "before\n");
    let host = host(&fixture, &[Capability::ReadNote]);

    assert!(!host.has_index(), "the fixture opened no index");
    refused(
        host.note_links("Launch.md").expect_err("links"),
        Capability::ReadIndex,
    );
    refused(host.folder_tags().expect_err("tags"), Capability::ReadIndex);
    refused(
        host.search_notes("before", 10).expect_err("search"),
        Capability::SearchNotes,
    );
}

#[test]
fn an_index_read_held_but_absent_says_the_index_is_not_there() {
    let fixture = fixture();
    write_note(&fixture, "Launch.md", "before\n");
    let host = host(&fixture, &[Capability::ReadIndex]);

    assert_eq!(
        host.note_links("Launch.md").expect_err("no index"),
        HostError::IndexUnavailable
    );
}

#[test]
fn list_notes_answers_in_path_order_and_stops_at_the_limit() {
    let fixture = fixture();
    write_note(&fixture, "Beta.md", "b\n");
    write_note(&fixture, "Alpha.md", "a\n");
    write_note(&fixture, "Projects/Writ.md", "w\n");
    write_note(&fixture, "Notes.txt", "a plain text note\n");
    write_note(&fixture, "diagram.png", "not a note\n");
    let host = host(&fixture, &[Capability::ListNotes]);

    let listed = host.list_notes(None, 10).expect("list");
    let names: Vec<&str> = listed.iter().map(|note| note.name.as_str()).collect();
    assert_eq!(names, ["Alpha", "Beta", "Notes.txt", "Writ"]);
    assert_eq!(listed[0].bytes, 2);

    assert_eq!(host.list_notes(None, 1).expect("list").len(), 1);
    let scoped = host.list_notes(Some("Projects"), 10).expect("list");
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].name, "Writ");
}

#[test]
fn read_note_answers_with_the_text_the_file_holds_and_its_hash() {
    let fixture = fixture();
    write_note(&fixture, "Launch.md", "---\ntag: one\n---\nbody\n");
    let host = host(&fixture, &[Capability::ReadNote]);

    let content = host.read_note("Launch.md").expect("read");
    assert_eq!(content.text, "---\ntag: one\n---\nbody\n");
    assert_eq!(content.bytes, content.text.len() as u64);
    assert_eq!(
        content.hash,
        writ_core::hash::sha256_hex(content.text.as_bytes())
    );
}

#[test]
fn a_note_that_is_not_utf8_is_answered_as_text_and_not_as_unreadable() {
    let fixture = fixture();
    let note = fixture.notes.join("Bytes.md");
    std::fs::write(&note, [0xff, 0xfe, 0x00]).expect("seed");
    let host = host(&fixture, &[Capability::ReadNote]);

    assert_eq!(
        host.read_note("Bytes.md").expect_err("not text"),
        HostError::NotText {
            path: "Bytes.md".to_string()
        }
    );
}

#[test]
fn note_summary_answers_the_length_without_reading_the_text() {
    let fixture = fixture();
    write_note(&fixture, "Launch.md", "before\n");
    let host = host(&fixture, &[Capability::ReadNote]);

    let summary = host.note_summary("Launch.md").expect("summary");
    assert_eq!(summary.name, "Launch");
    assert_eq!(summary.bytes, "before\n".len() as u64);
    assert!(summary.path.ends_with("Launch.md"));
}

#[test]
fn a_path_the_folder_does_not_hold_is_refused_once_the_capability_is_held() {
    let fixture = fixture();
    let outside = fixture.notes.parent().expect("a parent").join("outside.md");
    std::fs::write(&outside, "elsewhere\n").expect("seed");
    let host = host(&fixture, &[Capability::ReadNote]);

    assert_eq!(
        host.read_note(&outside.to_string_lossy())
            .expect_err("outside"),
        HostError::OutsideNotesFolder {
            path: outside.to_string_lossy().into_owned()
        }
    );
    assert_eq!(
        host.read_note("Missing.md").expect_err("missing"),
        HostError::NotFound {
            path: "Missing.md".to_string()
        }
    );
}

#[test]
fn write_note_lands_the_bytes_and_answers_with_the_new_hash() {
    let fixture = fixture();
    let note = write_note(&fixture, "Launch.md", "before\n");
    let host = host(&fixture, &[Capability::WriteNote]);

    let receipt = host
        .write_note("Launch.md", "after\n", LastKnown::Overwrite, origin())
        .expect("write");

    assert_eq!(
        std::fs::read_to_string(&note).expect("read back"),
        "after\n"
    );
    assert_eq!(receipt.bytes, "after\n".len() as u64);
    assert_eq!(receipt.hash, writ_core::hash::sha256_hex(b"after\n"));
}

#[test]
fn a_write_against_a_note_changed_underneath_is_refused_with_a_copy() {
    let fixture = fixture();
    let note = write_note(&fixture, "Launch.md", "before\n");
    let stale = writ_core::hash::sha256_bytes(b"what the caller read\n");
    let host = host(&fixture, &[Capability::WriteNote]);

    let error = host
        .write_note("Launch.md", "after\n", LastKnown::Hash(stale), origin())
        .expect_err("the note holds something else");

    match error {
        HostError::Conflict { conflict_copy, .. } => {
            assert!(conflict_copy.is_some(), "the text lands beside the note");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(&note).expect("read back"),
        "before\n"
    );
}

/// The capabilities a session that reads and writes holds.
const READ_AND_WRITE: &[Capability] = &[
    Capability::ReadNote,
    Capability::WriteNote,
    Capability::CreateNote,
    Capability::RenameNote,
];

#[test]
fn a_write_against_what_the_host_last_saw_lands_after_a_read() {
    let fixture = fixture();
    let note = write_note(&fixture, "Launch.md", "before\n");
    let host = host(&fixture, READ_AND_WRITE);

    host.read_note("Launch.md").expect("read");
    let receipt = host
        .write_note("Launch.md", "after\n", LastKnown::LastSeen, origin())
        .expect("the note still holds what was read");

    assert_eq!(
        std::fs::read_to_string(&note).expect("read back"),
        "after\n"
    );
    assert!(receipt.changed);
}

#[test]
fn a_note_the_host_has_not_seen_is_refused_and_left_alone() {
    let fixture = fixture();
    let note = write_note(&fixture, "Launch.md", "before\n");
    let before = modified(&note);
    let host = host(&fixture, READ_AND_WRITE);

    let error = host
        .write_note("Launch.md", "after\n", LastKnown::LastSeen, origin())
        .expect_err("there is nothing to compare the write with");

    assert_eq!(
        error,
        HostError::HashRequired {
            path: "Launch.md".to_string()
        }
    );
    assert_eq!(
        std::fs::read_to_string(&note).expect("read back"),
        "before\n"
    );
    assert_eq!(modified(&note), before);
    assert_eq!(folder_contents(&fixture.notes), ["Launch.md"]);
}

#[test]
fn a_path_the_folder_does_not_hold_keeps_its_own_answer_over_an_unseen_note() {
    let fixture = fixture();
    let host = host(&fixture, READ_AND_WRITE);

    assert_eq!(
        host.write_note("Missing.md", "after\n", LastKnown::LastSeen, origin())
            .expect_err("nothing is there"),
        HostError::NotFound {
            path: "Missing.md".to_string()
        }
    );
    assert!(matches!(
        host.write_note("../outside.md", "after\n", LastKnown::LastSeen, origin())
            .expect_err("out of the folder"),
        HostError::OutsideNotesFolder { .. }
    ));
}

#[test]
fn a_note_changed_since_the_host_read_it_is_refused_with_a_copy() {
    let fixture = fixture();
    let note = write_note(&fixture, "Launch.md", "as it was read\n");
    let host = host(&fixture, READ_AND_WRITE);
    host.read_note("Launch.md").expect("read");
    std::fs::write(&note, "as somebody else left it\n").expect("edit underneath");

    let error = host
        .write_note(
            "Launch.md",
            "what the caller sent\n",
            LastKnown::LastSeen,
            origin(),
        )
        .expect_err("the note moved on since it was read");

    let HostError::Conflict { conflict_copy, .. } = error else {
        panic!("expected a conflict, got {error:?}");
    };
    let copy = conflict_copy.expect("the text lands beside the note");
    assert_eq!(
        std::fs::read_to_string(copy).expect("read the copy"),
        "what the caller sent\n"
    );
    assert_eq!(
        std::fs::read_to_string(&note).expect("read back"),
        "as somebody else left it\n"
    );
}

#[test]
fn a_handle_derived_from_the_host_sees_what_the_host_read() {
    let fixture = fixture();
    let note = write_note(&fixture, "Launch.md", "before\n");
    let reading = host(&fixture, &[Capability::ReadNote]);
    let writing = reading.with_permissions(held(&[Capability::WriteNote]));

    reading.read_note("Launch.md").expect("read");
    writing
        .write_note("Launch.md", "after\n", LastKnown::LastSeen, origin())
        .expect("a read on one handle is a read on the other");

    assert_eq!(
        std::fs::read_to_string(&note).expect("read back"),
        "after\n"
    );
}

#[test]
fn a_second_host_over_the_same_folder_has_seen_nothing() {
    let fixture = fixture();
    write_note(&fixture, "Launch.md", "before\n");
    host(&fixture, READ_AND_WRITE)
        .read_note("Launch.md")
        .expect("read");

    assert!(matches!(
        host(&fixture, READ_AND_WRITE)
            .write_note("Launch.md", "after\n", LastKnown::LastSeen, origin())
            .expect_err("another session's read is not this one's"),
        HostError::HashRequired { .. }
    ));
}

#[test]
fn the_hosts_own_write_is_what_its_next_write_is_compared_with() {
    let fixture = fixture();
    let note = write_note(&fixture, "Launch.md", "before\n");
    let host = host(&fixture, READ_AND_WRITE);
    host.read_note("Launch.md").expect("read");

    host.write_note("Launch.md", "first\n", LastKnown::LastSeen, origin())
        .expect("first write");
    host.write_note("Launch.md", "second\n", LastKnown::LastSeen, origin())
        .expect("the note holds what the first write left");

    assert_eq!(
        std::fs::read_to_string(&note).expect("read back"),
        "second\n"
    );
    assert_eq!(folder_contents(&fixture.notes), ["Launch.md"]);
}

#[test]
fn a_note_the_host_minted_is_one_it_has_seen() {
    let fixture = fixture();
    let host = host(&fixture, READ_AND_WRITE);
    let minted = host
        .create_note("Ship it", "first\n", origin())
        .expect("mint");

    host.write_note(&minted.path, "second\n", LastKnown::LastSeen, origin())
        .expect("the host knows what it minted");

    assert_eq!(
        std::fs::read_to_string(fixture.notes.join("Ship it.md")).expect("read back"),
        "second\n"
    );
}

#[test]
fn a_renamed_note_keeps_what_the_host_saw_under_its_new_path() {
    let fixture = fixture();
    write_note(&fixture, "Writ.md", "body\n");
    let host = host(&fixture, READ_AND_WRITE);
    host.read_note("Writ.md").expect("read");
    let moved = host
        .rename_note("Writ.md", "Landed", origin())
        .expect("rename");

    host.write_note(&moved.path, "after\n", LastKnown::LastSeen, origin())
        .expect("a rename moves no byte");

    assert_eq!(
        std::fs::read_to_string(fixture.notes.join("Landed.md")).expect("read back"),
        "after\n"
    );
}

#[test]
fn an_overwrite_lands_on_a_note_the_host_has_not_seen() {
    let fixture = fixture();
    let note = write_note(&fixture, "Launch.md", "before\n");
    let host = host(&fixture, &[Capability::WriteNote]);

    host.write_note("Launch.md", "after\n", LastKnown::Overwrite, origin())
        .expect("an overwrite compares with nothing");

    assert_eq!(
        std::fs::read_to_string(&note).expect("read back"),
        "after\n"
    );
}

#[test]
fn create_note_mints_the_note_and_refuses_a_name_the_folder_holds() {
    let fixture = fixture();
    let host = host(&fixture, &[Capability::CreateNote]);

    let receipt = host
        .create_note("Ship it", "text\n", origin())
        .expect("mint");
    assert!(receipt.path.ends_with("Ship it.md"));
    assert!(fixture.notes.join("Ship it.md").is_file());

    assert!(matches!(
        host.create_note("Ship it", "other\n", origin())
            .expect_err("the name is taken"),
        HostError::NameTaken { .. }
    ));
    assert_eq!(
        std::fs::read_to_string(fixture.notes.join("Ship it.md")).expect("read back"),
        "text\n"
    );
}

#[test]
fn rename_note_moves_the_file_inside_its_folder() {
    let fixture = fixture();
    write_note(&fixture, "Projects/Writ.md", "body\n");
    let host = host(&fixture, &[Capability::RenameNote]);

    let receipt = host
        .rename_note("Projects/Writ.md", "Landed", origin())
        .expect("rename");

    assert!(receipt.path.ends_with("Projects/Landed.md") || receipt.path.ends_with("Landed.md"));
    assert!(receipt.previous_path.ends_with("Writ.md"));
    assert_eq!(receipt.bytes, "body\n".len() as u64);
    assert!(fixture.notes.join("Projects/Landed.md").is_file());
    assert!(!fixture.notes.join("Projects/Writ.md").exists());
}

#[test]
fn the_index_backed_methods_answer_from_the_walk() {
    let fixture = fixture();
    write_note(&fixture, "One.md", "#idea\n\nSee [[Two]].\n");
    write_note(&fixture, "Two.md", "---\nstatus: open\n---\nbody\n");
    build_index(&fixture);
    let host = indexed_host(
        &fixture,
        &[
            Capability::ReadIndex,
            Capability::SearchNotes,
            Capability::ReadNote,
        ],
    );

    assert_eq!(host.note_links("One.md").expect("links").len(), 1);
    assert_eq!(host.note_backlinks("Two.md").expect("backlinks").len(), 1);
    assert_eq!(
        host.note_facts("One.md").expect("facts").tags,
        vec![("idea".to_string(), 1)]
    );
    assert_eq!(
        host.note_facts("Two.md").expect("facts").properties.len(),
        1
    );
    assert!(host
        .folder_tags()
        .expect("tags")
        .iter()
        .any(|tag| tag.tag == "idea"));
    assert!(!host.search_notes("body", 10).expect("search").is_empty());
}

#[test]
fn a_second_handle_serves_another_set_over_the_same_folder_and_index() {
    let fixture = fixture();
    write_note(&fixture, "One.md", "#idea\n\nbody\n");
    build_index(&fixture);
    let reading = indexed_host(&fixture, &[Capability::ReadIndex]);

    let writing = reading.with_permissions(held(&[Capability::WriteNote]));

    assert!(
        writing.has_index(),
        "the second handle holds the same index"
    );
    assert_eq!(
        writing.notes_root(),
        resolve_for_containment(&fixture.notes)
            .expect("resolve the notes folder")
            .as_path(),
        "and the same folder"
    );
    assert!(reading.folder_tags().is_ok());
    refused(
        writing
            .folder_tags()
            .expect_err("the second set reads no index"),
        Capability::ReadIndex,
    );
    assert!(writing
        .write_note("One.md", "other\n", LastKnown::Overwrite, origin())
        .is_ok());
    refused(
        reading
            .write_note("One.md", "again\n", LastKnown::Overwrite, origin())
            .expect_err("the first set writes nothing"),
        Capability::WriteNote,
    );
}

#[test]
fn opening_a_folder_that_is_not_there_fails_rather_than_creating_one() {
    let dir = TempDir::new().expect("temp dir");
    let absent = dir.path().join("notes");

    let opened = NoteHostImpl::open(
        &absent,
        None,
        held(&[Capability::ListNotes]),
        FileExtension::Md,
    );

    assert!(matches!(opened, Err(HostError::NotFound { .. })));
    assert!(!absent.exists());
}

#[test]
fn opening_an_index_that_is_not_there_creates_no_database() {
    let fixture = fixture();

    let host = indexed_host(&fixture, &[Capability::ReadIndex]);

    assert!(!host.has_index());
    assert!(!fixture.db.exists(), "no database was created");
}

#[test]
fn an_identical_proposal_reports_no_change() {
    let fixture = fixture();
    let note = write_note(&fixture, "Launch.md", "the same text\n");
    let before = modified(&note);
    let host = host(&fixture, &[Capability::WriteNote]);

    let receipt = host
        .write_note(
            "Launch.md",
            "the same text\n",
            LastKnown::Overwrite,
            origin(),
        )
        .expect("a write of the text the note already holds is not a refusal");

    assert!(
        !receipt.changed,
        "a write that moved no bytes must not report a change"
    );
    assert_eq!(receipt.bytes, "the same text\n".len() as u64);
    assert_eq!(
        std::fs::read_to_string(&note).expect("read"),
        "the same text\n"
    );
    assert_eq!(modified(&note), before, "the file was rewritten");
}

#[test]
fn a_proposal_that_moves_bytes_reports_a_change() {
    let fixture = fixture();
    write_note(&fixture, "Launch.md", "before\n");
    let host = host(&fixture, &[Capability::WriteNote]);

    let receipt = host
        .write_note("Launch.md", "after\n", LastKnown::Overwrite, origin())
        .expect("the write lands");

    assert!(receipt.changed);
}

#[test]
fn a_host_mints_in_the_format_it_was_opened_with() {
    let fixture = fixture();
    let text = NoteHostImpl::open(
        &fixture.notes,
        None,
        held(&[Capability::CreateNote]),
        FileExtension::Txt,
    )
    .expect("open the host");

    let receipt = text
        .create_note("Ship it", "body\n", origin())
        .expect("the note is minted");

    assert!(receipt.path.ends_with("Ship it.txt"), "{}", receipt.path);
    assert!(fixture.notes.join("Ship it.txt").is_file());
}

#[test]
fn a_write_keeping_versions_keeps_the_ones_a_renamed_note_had_under_its_old_name() {
    use std::time::Duration;
    use writ_storage::identity::PlatformIdentity;
    use writ_storage::note_history::NoteHistoryStore;

    let fixture = fixture();
    write_note(&fixture, "Draft.md", "v1\n");
    let host = host(&fixture, &[Capability::WriteNote]);
    // Keyed against the folder as the host resolved it, the way the app and
    // `writ mcp` hand it to their stores.
    let notes = host.notes_root().to_path_buf();
    let note = notes.join("Draft.md");
    let store = NoteHistoryStore::open(&fixture._dir.path().join("data")).expect("open the store");
    store.set_notes_root(notes.clone());
    store.set_probe(std::sync::Arc::new(PlatformIdentity));
    // Two versions under the old name, so the text the write replaces is not
    // the whole of what the note had.
    let key = store.key_for(&note).expect("a key");
    let now = SystemTime::now();
    store
        .capture_replaced(&key, b"v0\n", now - Duration::from_secs(20))
        .expect("keep v0");
    store
        .capture_replaced(&key, b"v1\n", now - Duration::from_secs(10))
        .expect("keep v1");
    let renamed = notes.join("Final.md");
    std::fs::rename(&note, &renamed).expect("rename outside the host");

    host.with_history(Some(&store))
        .write_note("Final.md", "v2\n", LastKnown::Overwrite, origin())
        .expect("the write is made");

    let key = store.key_for(&renamed).expect("a key");
    let texts: Vec<Vec<u8>> = store
        .versions(&key)
        .expect("versions")
        .into_iter()
        .map(|entry| store.content(entry.id).expect("content"))
        .collect();
    assert_eq!(
        texts,
        [b"v2\n".to_vec(), b"v1\n".to_vec(), b"v0\n".to_vec()]
    );
}
