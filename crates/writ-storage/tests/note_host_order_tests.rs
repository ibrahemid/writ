//! What the note host does around a refusal:
//! a refused write with a history store attached keeps no version, and the
//! authority scanner's `#[cfg(test)]` cut is accurate for the files it reads.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use tempfile::TempDir;
use writ_core::notes::host::{Capability, HostError, NoteHost, PermissionSet};
use writ_core::notes::WriteOrigin;
use writ_storage::note_history::NoteHistoryStore;
use writ_storage::note_host::NoteHostImpl;

struct Fixture {
    _dir: TempDir,
    notes: PathBuf,
    writ: PathBuf,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().expect("temp dir");
    let notes = dir.path().join("notes");
    let writ = dir.path().join("writ");
    std::fs::create_dir_all(&notes).expect("notes folder");
    std::fs::create_dir_all(&writ).expect("writ folder");
    // The host resolves every path it hands on, so the history store has to be
    // told the same spelling of the folder or its keys never match.
    let notes = std::fs::canonicalize(&notes).expect("canonical notes folder");
    Fixture {
        _dir: dir,
        notes,
        writ,
    }
}

fn held(capabilities: &[Capability]) -> PermissionSet {
    capabilities.iter().copied().collect()
}

fn modified(file: &Path) -> Option<SystemTime> {
    std::fs::metadata(file).ok().and_then(|m| m.modified().ok())
}

fn folder_contents(notes: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(notes)
        .expect("read the folder")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The claim the shipped tests do not cover: a refused write on a host that
/// *does* hold a history store keeps no version and no conflict copy.
#[test]
fn a_refused_write_with_a_history_store_keeps_no_version() {
    let fixture = fixture();
    let note = fixture.notes.join("Launch.md");
    std::fs::write(&note, "before\n").expect("seed a note");

    let store = NoteHistoryStore::open(&fixture.writ).expect("history store");
    store.set_notes_root(fixture.notes.clone());
    let key = store.key_for(&note).expect("a key for the note");
    assert!(
        store.versions(&key).expect("versions").is_empty(),
        "the store starts empty"
    );

    let before_mtime = modified(&note);
    let before_bytes = std::fs::read(&note).expect("read back");

    // Everything but the one capability the write checks.
    let reading = NoteHostImpl::open(
        &fixture.notes,
        None,
        held(&[
            Capability::ListNotes,
            Capability::ReadNote,
            Capability::SearchNotes,
            Capability::ReadIndex,
            Capability::CreateNote,
            Capability::RenameNote,
        ]),
    )
    .expect("open the host")
    .with_history(Some(&store));

    let refused = reading
        .write_note("Launch.md", "after\n", None, WriteOrigin::Chat)
        .expect_err("a host without WriteNote has no write path");

    assert_eq!(
        refused,
        HostError::NotPermitted {
            capability: Capability::WriteNote
        }
    );
    assert_eq!(std::fs::read(&note).expect("read back"), before_bytes);
    assert_eq!(modified(&note), before_mtime, "the file was not touched");
    assert_eq!(
        folder_contents(&fixture.notes),
        vec!["Launch.md".to_string()],
        "no conflict copy was written"
    );
    assert!(
        store.versions(&key).expect("versions").is_empty(),
        "a refused write captured a version"
    );
}

/// The positive half, so the assertion above is not vacuous: the same host
/// holding `WriteNote` does capture one.
#[test]
fn an_allowed_write_through_the_host_captures_a_version() {
    let fixture = fixture();
    let note = fixture.notes.join("Launch.md");
    std::fs::write(&note, "before\n").expect("seed a note");

    let store = NoteHistoryStore::open(&fixture.writ).expect("history store");
    store.set_notes_root(fixture.notes.clone());
    let key = store.key_for(&note).expect("a key for the note");

    let writing = NoteHostImpl::open(&fixture.notes, None, held(&[Capability::WriteNote]))
        .expect("open the host")
        .with_history(Some(&store));
    writing
        .write_note("Launch.md", "after\n", None, WriteOrigin::Chat)
        .expect("the write lands");

    assert!(
        !store.versions(&key).expect("versions").is_empty(),
        "U8's hook is not installed by the host"
    );
    assert_eq!(
        std::fs::read_to_string(&note).expect("read back"),
        "after\n"
    );
}

/// A minted note has no earlier text, so `create_note` captures nothing even
/// with a store attached.
#[test]
fn a_minted_note_captures_no_version() {
    let fixture = fixture();
    let store = NoteHistoryStore::open(&fixture.writ).expect("history store");
    store.set_notes_root(fixture.notes.clone());

    let minting = NoteHostImpl::open(&fixture.notes, None, held(&[Capability::CreateNote]))
        .expect("open the host")
        .with_history(Some(&store));
    minting
        .create_note("Ship it", "body\r\n", WriteOrigin::Chat)
        .expect("the note is minted");

    let minted = fixture.notes.join("Ship it.md");
    assert!(minted.is_file());
    assert_eq!(
        std::fs::read_to_string(&minted).expect("read back"),
        "body\n",
        "a minted note lands as LF"
    );
    let key = store.key_for(&minted).expect("a key");
    assert!(
        store.versions(&key).expect("versions").is_empty(),
        "a minted note kept a version of text that never existed"
    );
}

/// Every method the empty set refuses, on arguments that would otherwise
/// produce a different answer: outside the folder, missing, and with no index.
#[test]
fn the_check_precedes_resolution_the_index_and_the_stat() {
    let fixture = fixture();
    std::fs::write(fixture.notes.join("Launch.md"), "before\n").expect("seed");
    let outside = fixture._dir.path().join("elsewhere.md");
    std::fs::write(&outside, "not a note\n").expect("seed outside");
    let outside = outside.to_string_lossy().into_owned();

    let nothing =
        NoteHostImpl::open(&fixture.notes, None, PermissionSet::default()).expect("open the host");
    assert!(!nothing.has_index());

    let refusals: Vec<(HostError, Capability)> = vec![
        (
            nothing.list_notes(None, 10).unwrap_err(),
            Capability::ListNotes,
        ),
        (
            nothing.read_note(&outside).unwrap_err(),
            Capability::ReadNote,
        ),
        (
            nothing.read_note("Gone.md").unwrap_err(),
            Capability::ReadNote,
        ),
        (
            nothing.note_summary(&outside).unwrap_err(),
            Capability::ReadNote,
        ),
        (
            nothing.search_notes("body", 10).unwrap_err(),
            Capability::SearchNotes,
        ),
        (
            nothing.note_links(&outside).unwrap_err(),
            Capability::ReadIndex,
        ),
        (
            nothing.note_backlinks("Gone.md").unwrap_err(),
            Capability::ReadIndex,
        ),
        (
            nothing.note_facts(&outside).unwrap_err(),
            Capability::ReadIndex,
        ),
        (nothing.folder_tags().unwrap_err(), Capability::ReadIndex),
        (
            nothing
                .write_note(&outside, "x", None, WriteOrigin::Chat)
                .unwrap_err(),
            Capability::WriteNote,
        ),
        (
            nothing
                .create_note("Ship it", "x", WriteOrigin::Chat)
                .unwrap_err(),
            Capability::CreateNote,
        ),
        (
            nothing
                .rename_note(&outside, "Landed", WriteOrigin::Chat)
                .unwrap_err(),
            Capability::RenameNote,
        ),
    ];

    for (error, capability) in refusals {
        assert_eq!(error, HostError::NotPermitted { capability });
    }
    assert_eq!(
        std::fs::read_to_string(&outside).expect("read back"),
        "not a note\n"
    );
    assert_eq!(
        folder_contents(&fixture.notes),
        vec!["Launch.md".to_string()]
    );
}

/// The authority scanner cuts a file at its first `#[cfg(test)]`. That is right
/// only while no shipped code sits after one. This reads the same files it does.
#[test]
fn the_authority_cut_loses_no_shipped_code() {
    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root")
            .to_path_buf()
    }
    fn scanned(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).expect("read a source folder") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                found.extend(scanned(&path));
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found.push(path);
            }
        }
        found
    }

    let mut files = scanned(&workspace_root().join("crates/writ-mcp/src"));
    files.push(workspace_root().join("src-tauri/src/commands/chat.rs"));

    for file in files {
        let text = std::fs::read_to_string(&file).expect("read a source file");
        let Some(at) = text.find("#[cfg(test)]") else {
            continue;
        };
        // Everything the scanner drops has to be test code. A top-level item
        // that is not a `mod`/`use` behind the attribute would be missed.
        let dropped = &text[at..];
        let shipped_items: Vec<&str> = dropped
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                line.len() == trimmed.len()
                    && (trimmed.starts_with("pub fn ")
                        || trimmed.starts_with("fn ")
                        || trimmed.starts_with("pub struct ")
                        || trimmed.starts_with("pub async fn ")
                        || trimmed.starts_with("impl "))
            })
            .collect();
        assert!(
            shipped_items.is_empty(),
            "{} has shipped items after its first #[cfg(test)], which the authority scanner drops: {shipped_items:?}",
            file.display()
        );
    }
}
