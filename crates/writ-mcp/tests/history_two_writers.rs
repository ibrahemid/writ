//! The app and `writ mcp` writing one version store at the same moment.
//!
//! Two processes, because that is what ships: the app holds `history.db` open
//! for as long as it runs, and a connected program's writes land through a
//! second process the program started. This test binary is started a second
//! time to be that process, so the two connections are in different processes
//! and the database's own locking is what keeps them apart.
//!
//! Each side writes its own note, so what each one should have left is exact:
//! one entry for the text the note started with and one for every write.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;
use writ_core::notes::identity::{FileIdentity, IdentityProbe};
use writ_core::notes::WriteOrigin;
use writ_mcp::consent::{ClientId, ConfigGate};
use writ_mcp::tools::{FixedAppFolder, ToolHost};
use writ_storage::buffer_store::read_disk_state;
use writ_storage::guarded::{
    keep_versions, write_note_guarded, ConflictPolicy, DiskRead, GuardedWrite, WriteCapture,
};
use writ_storage::note_history::NoteHistoryStore;

/// Names the data folder when this binary is the server side.
const SERVER_SIDE: &str = "WRIT_TEST_HISTORY_SERVER_SIDE";

/// This test's own name, which the second process is started with.
const TEST_NAME: &str = "the_app_and_the_server_keep_every_version_while_writing_at_once";

/// How many writes each side makes.
const WRITES: usize = 40;

/// The note the app writes.
const APP_NOTE: &str = "App.md";

/// The note the server writes.
const SERVER_NOTE: &str = "Server.md";

/// How long either side waits for the other to reach the starting line.
const START_TIMEOUT: Duration = Duration::from_secs(60);

/// The platform's answer on Unix, the way the app's own probe reads it, so the
/// app side keys its notes the way the running app does.
struct Probe;

impl IdentityProbe for Probe {
    #[cfg(unix)]
    fn identity_of(&self, path: &Path) -> Option<FileIdentity> {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path).ok()?;
        metadata.is_file().then(|| FileIdentity::Inode {
            dev: metadata.dev(),
            ino: metadata.ino(),
            birth_ns: None,
        })
    }

    #[cfg(not(unix))]
    fn identity_of(&self, _path: &Path) -> Option<FileIdentity> {
        None
    }
}

fn notes_of(writ: &Path) -> PathBuf {
    writ.join("notes")
}

fn app_text(write: usize) -> String {
    format!("the app's text, write {write}\n")
}

fn server_text(write: usize) -> String {
    format!("the server's text, write {write}\n")
}

/// Waits for `path` to exist, failing the test after [`START_TIMEOUT`].
fn wait_for(path: &Path) {
    let deadline = Instant::now() + START_TIMEOUT;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "{} never appeared",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The store the app opens, set up the way `src-tauri/src/state.rs` sets it.
fn app_store(writ: &Path) -> NoteHistoryStore {
    let store = NoteHistoryStore::open(writ).expect("open the version store");
    store.set_notes_root(notes_of(writ));
    store.set_probe(std::sync::Arc::new(Probe));
    store
}

/// The texts Revert To lists for `name`, oldest first.
fn listed_versions(store: &NoteHistoryStore, notes: &Path, name: &str) -> Vec<String> {
    let key = store
        .key_for(&notes.join(name))
        .expect("a note in the folder has a key");
    let mut texts: Vec<String> = store
        .versions(&key)
        .expect("read the versions")
        .into_iter()
        .map(|entry| {
            String::from_utf8(store.content(entry.id).expect("a version's text")).expect("utf-8")
        })
        .collect();
    texts.reverse();
    texts
}

/// The second process: `writ mcp`'s tool host, writing its note through the
/// tools a connected program calls.
fn run_server_side(writ: &Path) {
    let notes = notes_of(writ);
    let host = ToolHost::open(
        &notes,
        &writ.join("writ.db"),
        writ,
        Box::new(FixedAppFolder(notes.clone())),
        Box::new(ConfigGate::new(writ)),
    )
    .expect("open the tool host");
    let client = ClientId::named("Test Client");
    let mut hash = host
        .read_note(&client, SERVER_NOTE)
        .expect("read the note")
        .hash;

    std::fs::write(writ.join("server-ready"), "").expect("say ready");
    wait_for(&writ.join("go"));

    for write in 1..=WRITES {
        let receipt = host
            .write_note(
                &client,
                SERVER_NOTE,
                &server_text(write),
                Some(&hash),
                false,
            )
            .unwrap_or_else(|error| panic!("server write {write} was refused: {error}"));
        hash = receipt.hash;
    }
}

#[test]
fn the_app_and_the_server_keep_every_version_while_writing_at_once() {
    if let Ok(writ) = std::env::var(SERVER_SIDE) {
        run_server_side(Path::new(&writ));
        return;
    }

    let dir = TempDir::new().expect("temp dir");
    let writ = dir.path().to_path_buf();
    let notes = notes_of(&writ);
    std::fs::create_dir_all(&notes).expect("notes folder");
    std::fs::write(notes.join(APP_NOTE), app_text(0)).expect("seed the app's note");
    std::fs::write(notes.join(SERVER_NOTE), server_text(0)).expect("seed the server's note");
    std::fs::write(
        writ.join("config.toml"),
        "[mcp]\nenabled = true\n\n[[mcp.approved_clients]]\nname = \"Test Client\"\nread = true\nwrite = true\n",
    )
    .expect("approve the test client");

    // The app is running before the program connects, as it is in use.
    let store = app_store(&writ);
    let keep = keep_versions(&store);
    let app_note = notes.join(APP_NOTE);
    let mut last_known = read_disk_state(&app_note).expect("read the app's note");

    let server = Command::new(std::env::current_exe().expect("this test binary"))
        .args([TEST_NAME, "--exact", "--nocapture", "--test-threads=1"])
        .env(SERVER_SIDE, &writ)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the server side");

    wait_for(&writ.join("server-ready"));
    std::fs::write(writ.join("go"), "").expect("start both sides");

    for write in 1..=WRITES {
        let text = app_text(write);
        let outcome = write_note_guarded(
            GuardedWrite {
                target: &app_note,
                bytes: text.as_bytes(),
                last_known,
                on_disk: DiskRead::Fresh,
                dataless: None,
                origin: WriteOrigin::Chat,
                on_conflict: ConflictPolicy::RefuseWithCopy,
                history: Some(&keep as &dyn Fn(WriteCapture<'_>)),
            },
            None,
        )
        .unwrap_or_else(|error| panic!("app write {write} was refused: {error}"));
        last_known = Some(outcome.disk_state);
    }

    let finished = server.wait_with_output().expect("wait for the server side");
    assert!(
        finished.status.success(),
        "the server side failed:\n{}\n{}",
        String::from_utf8_lossy(&finished.stdout),
        String::from_utf8_lossy(&finished.stderr)
    );

    let app_view = app_store(&writ);
    assert_eq!(
        listed_versions(&app_view, &notes, APP_NOTE),
        (0..=WRITES).map(app_text).collect::<Vec<_>>(),
        "every text the app's note held is listed"
    );
    assert_eq!(
        listed_versions(&app_view, &notes, SERVER_NOTE),
        (0..=WRITES).map(server_text).collect::<Vec<_>>(),
        "every text the server's note held is listed"
    );
}
