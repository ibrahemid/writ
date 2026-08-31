//! The disk half of the data-folder guard: finding the Syncthing marker the
//! policy in `writ-core` is handed, and what the launch does with the verdict
//! (ADR-028 §8).

use std::path::PathBuf;

use tempfile::TempDir;
use writ_core::startup::{classify_data_dir, DataDirVerdict, Platform, SyncProvider};
use writ_tauri_lib::startup::{data_dir_verdict, stfolder_markers, HOST_PLATFORM};
use writ_tauri_lib::state::{AppState, NotesRootFallbackReason};

#[test]
fn stfolder_marker_is_found_on_an_ancestor() {
    let root = TempDir::new().expect("temp dir");
    let synced = root.path().join("Sync");
    let data_dir = synced.join("notes").join(".writ");
    std::fs::create_dir_all(&data_dir).expect("data dir");
    std::fs::create_dir_all(synced.join(".stfolder")).expect("marker");

    let markers = stfolder_markers(&data_dir);
    assert_eq!(markers, vec![synced.clone()]);

    let verdict = classify_data_dir(Platform::Linux, &data_dir, None, None, &markers);
    assert_eq!(
        verdict,
        DataDirVerdict::InsideSyncProvider {
            provider: SyncProvider::Syncthing,
            root: synced,
        }
    );
}

#[test]
fn a_directory_with_no_marker_above_it_yields_none() {
    let root = TempDir::new().expect("temp dir");
    let data_dir = root.path().join("plain").join(".writ");
    std::fs::create_dir_all(&data_dir).expect("data dir");

    assert_eq!(stfolder_markers(&data_dir), Vec::<PathBuf>::new());
}

#[test]
fn a_marker_beside_the_data_directory_itself_is_found() {
    let root = TempDir::new().expect("temp dir");
    let data_dir = root.path().join(".writ");
    std::fs::create_dir_all(data_dir.join(".stfolder")).expect("marker");

    assert_eq!(stfolder_markers(&data_dir), vec![data_dir]);
}

/// Both notes-folder cases in one test, and one file: `AppState::initialize`
/// reads `WRIT_DATA_DIR` and `WRIT_NOTES_DIR` from the process environment, so
/// two tests setting them would race each other.
///
/// The launch must keep `<data folder>/Writ`, which is what
/// `resolve_notes_root_from` picks whenever `WRIT_DATA_DIR` is in force: a
/// guard that turned that down would stop every instance running against its
/// own data folder. A notes folder anywhere else inside the data folder is
/// turned down by `usable_notes_root` before the guard sees it and the launch
/// falls back to the default, so the guard in `initialize` is the backstop
/// behind that rule rather than the rule itself.
#[test]
fn the_launch_keeps_its_notes_beside_its_own_data_folder() {
    let kept = TempDir::new().expect("data folder");
    std::env::set_var("WRIT_DATA_DIR", kept.path());
    std::env::set_var("WRIT_NOTES_DIR", kept.path().join("Writ"));
    let state = AppState::initialize().expect("the default notes folder must not stop the launch");
    std::env::remove_var("WRIT_NOTES_DIR");
    std::env::remove_var("WRIT_DATA_DIR");

    assert_eq!(
        state.notes_root(),
        std::fs::canonicalize(kept.path().join("Writ")).expect("notes folder")
    );
    assert_eq!(state.notes_root_fallback(), None);
    drop(state);

    let turned_down = TempDir::new().expect("data folder");
    std::env::set_var("WRIT_DATA_DIR", turned_down.path());
    std::env::set_var("WRIT_NOTES_DIR", turned_down.path().join("notes"));
    let state = AppState::initialize().expect("the launch falls back rather than stopping");
    std::env::remove_var("WRIT_NOTES_DIR");
    std::env::remove_var("WRIT_DATA_DIR");

    assert_eq!(
        state.notes_root(),
        std::fs::canonicalize(turned_down.path().join("Writ")).expect("notes folder")
    );
    assert_eq!(
        state.notes_root_fallback().map(|fallback| fallback.reason),
        Some(NotesRootFallbackReason::HoldsWritData)
    );
}

#[test]
fn the_verdict_carries_the_marker_walk_to_the_policy() {
    let root = TempDir::new().expect("temp dir");
    let synced = root.path().join("Sync");
    let data_dir = synced.join("notes").join(".writ");
    std::fs::create_dir_all(&data_dir).expect("data dir");
    std::fs::create_dir_all(synced.join(".stfolder")).expect("marker");

    assert_eq!(
        data_dir_verdict(&data_dir, None, None),
        DataDirVerdict::InsideSyncProvider {
            provider: SyncProvider::Syncthing,
            // Resolved, because that is the folder the database would land in.
            root: std::fs::canonicalize(&synced).expect("synced folder"),
        }
    );

    let plain = root.path().join("plain").join(".writ");
    std::fs::create_dir_all(&plain).expect("data dir");
    assert_eq!(data_dir_verdict(&plain, None, None), DataDirVerdict::Ok);
}

/// The case the as-spelled path alone cannot see: `~/.writ` is a symlink into
/// a synced folder, so only the canonical spelling says where the database
/// would actually land.
#[cfg(unix)]
#[test]
fn a_data_folder_symlinked_into_a_synced_folder_is_refused() {
    let root = TempDir::new().expect("temp dir");
    // Canonical, because the temporary directory is reached through a symlink
    // on macOS and the home prefix has to be the spelling the canonical data
    // folder shares.
    let home = std::fs::canonicalize(root.path()).expect("home");
    let synced = home.join("Dropbox");
    std::fs::create_dir_all(synced.join("real")).expect("synced folder");

    let spelled = root.path().join(".writ");
    std::os::unix::fs::symlink(synced.join("real"), &spelled).expect("symlink");

    assert_eq!(
        classify_data_dir(HOST_PLATFORM, &spelled, Some(&home), None, &[]),
        DataDirVerdict::Ok,
        "the spelling alone hides the synced folder, which is why both are asked"
    );
    assert_eq!(
        data_dir_verdict(&spelled, Some(&home), None),
        DataDirVerdict::InsideSyncProvider {
            provider: SyncProvider::Dropbox,
            root: synced,
        }
    );
}

/// The case the verifier reproduced: a data folder Writ has not created yet.
///
/// `canonicalize` fails on a path that does not exist, so a first launch has
/// only the planned path to go on, and the guard has to judge where that path
/// will land. Here the leaf is missing and the folder above it is a symlink
/// to the synced folder, which no spelling of the path shows.
///
/// The guard must also leave the folder alone: it runs before `create_dir_all`
/// in `AppState::initialize`, so nothing may appear under the synced tree.
#[cfg(unix)]
#[test]
fn a_data_folder_writ_has_not_created_yet_is_refused() {
    let root = TempDir::new().expect("temp dir");
    let home = std::fs::canonicalize(root.path()).expect("home");
    let synced = home.join("Dropbox");
    std::fs::create_dir_all(&synced).expect("synced folder");

    let spelled = home.join("sync-link");
    std::os::unix::fs::symlink(&synced, &spelled).expect("symlink");
    let planned = spelled.join("newdata");

    assert_eq!(
        classify_data_dir(HOST_PLATFORM, &planned, Some(&home), None, &[]),
        DataDirVerdict::Ok,
        "no spelling of the path shows the synced folder, which is why it is resolved"
    );
    assert_eq!(
        data_dir_verdict(&planned, Some(&home), None),
        DataDirVerdict::InsideSyncProvider {
            provider: SyncProvider::Dropbox,
            root: synced.clone(),
        }
    );
    assert_eq!(
        std::fs::read_dir(&synced).expect("synced folder").count(),
        0,
        "the guard must not create the folder it turns down"
    );
}

/// A folder inside the synced tree that does not exist yet is refused by its
/// planned location, and the guard leaves the tree empty.
#[test]
fn the_guard_creates_nothing_at_the_folder_it_turns_down() {
    let root = TempDir::new().expect("temp dir");
    let home = std::fs::canonicalize(root.path()).expect("home");
    let synced = home.join("Dropbox");
    std::fs::create_dir_all(&synced).expect("synced folder");
    let planned = synced.join("newdata");

    assert_eq!(
        data_dir_verdict(&planned, Some(&home), None),
        DataDirVerdict::InsideSyncProvider {
            provider: SyncProvider::Dropbox,
            root: synced.clone(),
        }
    );
    assert!(!planned.exists());
    assert_eq!(
        std::fs::read_dir(&synced).expect("synced folder").count(),
        0
    );
}
