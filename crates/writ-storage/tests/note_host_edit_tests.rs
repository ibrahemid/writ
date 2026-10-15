//! The writes that keep a note's shape: an append, a replace in the note's own
//! encoding and line ending, and a passage replaced in place.

use std::path::PathBuf;
use std::time::Duration;

use tempfile::TempDir;
use writ_core::config::FileExtension;
use writ_core::hash::digest_from_hex;
use writ_core::notes::host::{Capability, HostError, LastKnown, NoteHost};
use writ_core::notes::WriteOrigin;
use writ_storage::identity::PlatformIdentity;
use writ_storage::note_history::NoteHistoryStore;
use writ_storage::note_host::NoteHostImpl;

struct Fixture {
    dir: TempDir,
    notes: PathBuf,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().expect("temp dir");
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("notes folder");
    Fixture { dir, notes }
}

fn host(fixture: &Fixture) -> NoteHostImpl<'static> {
    NoteHostImpl::open(
        &fixture.notes,
        None,
        [Capability::ReadNote, Capability::WriteNote]
            .into_iter()
            .collect(),
        FileExtension::Txt,
    )
    .expect("open the host")
}

fn store(fixture: &Fixture, host: &NoteHostImpl<'_>) -> NoteHistoryStore {
    let store = NoteHistoryStore::open(&fixture.dir.path().join("data")).expect("open the store");
    store.set_notes_root(host.notes_root().to_path_buf());
    store.set_probe(std::sync::Arc::new(PlatformIdentity));
    store
}

fn cli() -> WriteOrigin {
    WriteOrigin::Cli
}

#[test]
fn an_append_lands_on_a_line_of_its_own_in_the_notes_line_ending() {
    let fixture = fixture();
    let file = fixture.notes.join("Plan.txt");
    std::fs::write(&file, "one\r\ntwo").expect("seed");
    let receipt = host(&fixture)
        .append_note("Plan.txt", "three\n", cli())
        .expect("append");
    assert!(receipt.changed);
    assert_eq!(
        std::fs::read(&file).expect("read"),
        b"one\r\ntwo\r\nthree\r\n"
    );
}

#[test]
fn an_append_keeps_a_version_of_what_the_note_held() {
    let fixture = fixture();
    let file = fixture.notes.join("Plan.txt");
    std::fs::write(&file, "before\n").expect("seed");
    let host = host(&fixture);
    let store = store(&fixture, &host);
    let note = host.notes_root().join("Plan.txt");
    host.with_history(Some(&store))
        .append_note("Plan.txt", "hi", cli())
        .expect("append");
    let key = store.key_for(&note).expect("key");
    let texts: Vec<Vec<u8>> = store
        .versions(&key)
        .expect("versions")
        .into_iter()
        .map(|entry| store.content(entry.id).expect("content"))
        .collect();
    assert!(texts.contains(&b"before\n".to_vec()), "{texts:?}");
}

#[test]
fn an_append_writes_in_the_notes_own_encoding() {
    let fixture = fixture();
    let file = fixture.notes.join("Latin.txt");
    // "café" in windows-1252, long enough for the detector to settle.
    let mut seeded = b"Le caf\xe9 est ouvert. Le caf\xe9 est ferm\xe9.\n".to_vec();
    seeded.extend_from_slice(b"Une r\xe9union demain matin.\n");
    std::fs::write(&file, &seeded).expect("seed");
    host(&fixture)
        .append_note("Latin.txt", "D\u{e9}j\u{e0} vu", cli())
        .expect("append");
    let mut expected = seeded.clone();
    expected.extend_from_slice(b"D\xe9j\xe0 vu\n");
    assert_eq!(std::fs::read(&file).expect("read"), expected);
}

#[test]
fn an_append_the_encoding_cannot_hold_writes_nothing() {
    let fixture = fixture();
    let file = fixture.notes.join("Latin.txt");
    let seeded = b"Le caf\xe9 est ouvert. Le caf\xe9 est ferm\xe9.\nUne r\xe9union.\n".to_vec();
    std::fs::write(&file, &seeded).expect("seed");
    let error = host(&fixture)
        .append_note("Latin.txt", "\u{1F600}", cli())
        .expect_err("refused");
    assert!(
        matches!(&error, HostError::EncodingLossy { characters, .. } if characters.contains("U+1F600")),
        "{error:?}"
    );
    assert_eq!(std::fs::read(&file).expect("read"), seeded);
}

#[test]
fn an_append_of_nothing_writes_nothing() {
    let fixture = fixture();
    let file = fixture.notes.join("Plan.txt");
    std::fs::write(&file, "kept\n").expect("seed");
    let before = std::fs::metadata(&file).expect("meta").modified().ok();
    std::thread::sleep(Duration::from_millis(20));
    let receipt = host(&fixture)
        .append_note("Plan.txt", " \n\n", cli())
        .expect("append");
    assert!(!receipt.changed);
    assert_eq!(std::fs::read(&file).expect("read"), b"kept\n");
    assert_eq!(
        std::fs::metadata(&file).expect("meta").modified().ok(),
        before
    );
}

#[test]
fn an_append_to_a_file_outside_the_folder_is_refused() {
    let fixture = fixture();
    let outside = fixture.dir.path().join("Outside.txt");
    std::fs::write(&outside, "kept\n").expect("seed");
    let error = host(&fixture)
        .append_note(&outside.to_string_lossy(), "more", cli())
        .expect_err("refused");
    assert!(
        matches!(error, HostError::OutsideNotesFolder { .. }),
        "{error:?}"
    );
    assert_eq!(std::fs::read(&outside).expect("read"), b"kept\n");
}

#[test]
fn a_replace_keeps_the_notes_line_ending_and_encoding() {
    let fixture = fixture();
    let file = fixture.notes.join("Latin.txt");
    let seeded = b"Le caf\xe9 est ouvert.\r\nUne r\xe9union demain.\r\n".to_vec();
    std::fs::write(&file, &seeded).expect("seed");
    let host = host(&fixture);
    let read = host.read_note("Latin.txt").expect("read");
    let hash = digest_from_hex(&read.hash).expect("hash");
    let receipt = host
        .replace_note(
            "Latin.txt",
            "Le caf\u{e9} est ferm\u{e9}.\nUne r\u{e9}union demain.\n",
            LastKnown::Hash(hash),
            cli(),
        )
        .expect("replace");
    assert!(receipt.changed);
    assert_eq!(
        std::fs::read(&file).expect("read"),
        b"Le caf\xe9 est ferm\xe9.\r\nUne r\xe9union demain.\r\n"
    );
}

#[test]
fn a_replace_against_a_note_that_moved_on_is_refused_with_a_copy() {
    let fixture = fixture();
    let file = fixture.notes.join("Plan.txt");
    std::fs::write(&file, "first\n").expect("seed");
    let host = host(&fixture);
    let read = host.read_note("Plan.txt").expect("read");
    let hash = digest_from_hex(&read.hash).expect("hash");
    std::fs::write(&file, "edited elsewhere\n").expect("edit");
    let error = host
        .replace_note("Plan.txt", "rewritten\n", LastKnown::Hash(hash), cli())
        .expect_err("refused");
    assert!(
        matches!(
            &error,
            HostError::Conflict {
                conflict_copy: Some(_),
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(std::fs::read(&file).expect("read"), b"edited elsewhere\n");
}

#[test]
fn a_passage_is_replaced_in_place_and_the_rest_of_the_note_is_left() {
    let fixture = fixture();
    let file = fixture.notes.join("Plan.txt");
    std::fs::write(&file, "one\r\ntwo\r\nthree\r\n").expect("seed");
    let receipt = host(&fixture)
        .replace_in_note("Plan.txt", "two\nthree", "2\n3", cli())
        .expect("replace");
    assert!(receipt.written.changed);
    assert_eq!(receipt.line, 2);
    assert_eq!(std::fs::read(&file).expect("read"), b"one\r\n2\r\n3\r\n");
}

#[test]
fn a_passage_the_note_does_not_hold_writes_nothing() {
    let fixture = fixture();
    let file = fixture.notes.join("Plan.txt");
    std::fs::write(&file, "one\ntwo\none\n").expect("seed");
    let host = host(&fixture);
    assert_eq!(
        host.replace_in_note("Plan.txt", "three", "3", cli()),
        Err(HostError::PassageAbsent {
            path: "Plan.txt".to_string()
        })
    );
    assert_eq!(
        host.replace_in_note("Plan.txt", "one", "1", cli()),
        Err(HostError::PassageRepeated {
            path: "Plan.txt".to_string(),
            count: 2
        })
    );
    assert_eq!(
        host.replace_in_note("Plan.txt", "", "1", cli()),
        Err(HostError::PassageEmpty {
            path: "Plan.txt".to_string()
        })
    );
    assert_eq!(std::fs::read(&file).expect("read"), b"one\ntwo\none\n");
}

#[test]
fn a_passage_replace_keeps_a_version_of_what_the_note_held() {
    let fixture = fixture();
    std::fs::write(fixture.notes.join("Plan.txt"), "before\n").expect("seed");
    let host = host(&fixture);
    let store = store(&fixture, &host);
    let note = host.notes_root().join("Plan.txt");
    host.with_history(Some(&store))
        .replace_in_note("Plan.txt", "before", "after", cli())
        .expect("replace");
    let key = store.key_for(&note).expect("key");
    let texts: Vec<Vec<u8>> = store
        .versions(&key)
        .expect("versions")
        .into_iter()
        .map(|entry| store.content(entry.id).expect("content"))
        .collect();
    assert!(texts.contains(&b"before\n".to_vec()), "{texts:?}");
}

#[test]
fn a_passage_replace_writes_in_the_notes_own_encoding() {
    let fixture = fixture();
    let file = fixture.notes.join("Latin.txt");
    let seeded =
        b"Le caf\xe9 est ouvert. Le caf\xe9 est ferm\xe9.\nUne r\xe9union demain matin.\n".to_vec();
    std::fs::write(&file, &seeded).expect("seed");
    host(&fixture)
        .replace_in_note(
            "Latin.txt",
            "demain matin",
            "d\u{e9}j\u{e0} pass\u{e9}e",
            cli(),
        )
        .expect("replace");
    assert_eq!(
        std::fs::read(&file).expect("read"),
        b"Le caf\xe9 est ouvert. Le caf\xe9 est ferm\xe9.\nUne r\xe9union d\xe9j\xe0 pass\xe9e.\n"
    );
}

#[test]
fn a_passage_replace_needs_the_write_capability() {
    let fixture = fixture();
    std::fs::write(fixture.notes.join("Plan.txt"), "one\n").expect("seed");
    let reader = host(&fixture).with_permissions([Capability::ReadNote].into_iter().collect());
    assert!(matches!(
        reader.replace_in_note("Plan.txt", "one", "1", cli()),
        Err(HostError::NotPermitted { .. })
    ));
}

#[test]
fn a_heading_is_found_on_its_line_without_counting_as_a_read() {
    let fixture = fixture();
    std::fs::write(
        fixture.notes.join("Plan.md"),
        "# Plan\nintro\n\n## Next steps\nwork\n",
    )
    .expect("seed");
    let host = host(&fixture);

    assert_eq!(host.heading_line("Plan.md", "Next steps"), Ok(Some(4)));
    assert_eq!(host.heading_line("Plan.md", "Missing"), Ok(None));
    assert!(matches!(
        host.heading_line("../outside.md", "Plan"),
        Err(HostError::OutsideNotesFolder { .. }) | Err(HostError::NotFound { .. })
    ));

    let unread = host.write_note("Plan.md", "new\n", LastKnown::LastSeen, cli());
    assert!(
        matches!(unread, Err(HostError::HashRequired { .. })),
        "{unread:?}"
    );
}
