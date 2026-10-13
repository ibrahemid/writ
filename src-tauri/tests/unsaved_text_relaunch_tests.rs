//! Text a file refused, carried across an exit that ran nothing on the way out.
//!
//! The person types into a note whose file is read-only, every save is
//! refused, and the process is killed or the machine loses power. Nothing
//! that runs at a quit runs, so whatever survives was written before the end.
//! The heartbeat is given its beat first, as it would have had one in the two
//! minutes before the end.
//!
//! One test in its own binary, because `initialize` reads `WRIT_DATA_DIR` and
//! `WRIT_NOTES_DIR` from the process environment and a second test in the same
//! binary would race it.

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use writ_tauri_lib::commands::buffer::{save_buffer_content_inner, ERR_READ_ONLY_DESTINATION};
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::heartbeat;
use writ_tauri_lib::security::canonicalize_for_authorization;
use writ_tauri_lib::state::AppState;

const ON_DISK: &str = "what the file held\n";
const TYPED: &str = "what the file held\ntyped after the file was locked\n";

fn recovered_copies(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("read_dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().contains("(recovered "))
        })
        .collect();
    found.sort();
    found
}

fn set_read_only(path: &Path, read_only: bool) {
    let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
    permissions.set_readonly(read_only);
    std::fs::set_permissions(path, permissions).expect("set permissions");
}

#[test]
fn text_a_read_only_file_refused_survives_an_unclean_exit() {
    let data = TempDir::new().expect("data folder");
    let notes = TempDir::new().expect("notes folder");
    std::env::set_var("WRIT_DATA_DIR", data.path());
    std::env::set_var("WRIT_NOTES_DIR", notes.path());

    let session = AppState::initialize().expect("the first launch");
    let note = session.notes_root().join("Locked.md");
    std::fs::write(&note, ON_DISK).expect("write the note");
    let canonical = canonicalize_for_authorization(&note).expect("canonical");
    session.authorized_paths.record_for_open(canonical.clone());
    let id = open_file_from_path(&session, &canonical)
        .expect("open")
        .doc
        .expect("the file opened")
        .id;

    set_read_only(&note, true);
    let refused = save_buffer_content_inner(&session, &id, TYPED)
        .expect_err("a read-only file refuses the save");
    assert!(refused.starts_with(ERR_READ_ONLY_DESTINATION), "{refused}");

    heartbeat::beat(&session);
    drop(session);

    let relaunched = AppState::initialize();
    std::env::remove_var("WRIT_DATA_DIR");
    std::env::remove_var("WRIT_NOTES_DIR");
    let relaunched = relaunched.expect("the relaunch");
    set_read_only(&note, false);

    let offered = relaunched
        .recovered_buffers
        .lock()
        .expect("recovered buffers")
        .iter()
        .find(|buf| buf.id == id)
        .cloned()
        .expect("the relaunch offers the text the file refused");
    assert!(!offered.removed_on_disk);
    assert_eq!(offered.content, TYPED);

    assert_eq!(
        std::fs::read_to_string(&note).expect("read the note"),
        ON_DISK,
        "the read-only file is left as it was"
    );
    let notes_root = relaunched.notes_root();
    let copies = recovered_copies(&notes_root);
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert_eq!(
        std::fs::read_to_string(&copies[0]).expect("read the copy"),
        TYPED,
        "the text the person typed is on disk beside the note"
    );
}
