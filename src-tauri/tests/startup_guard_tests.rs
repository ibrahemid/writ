//! The disk half of the data-folder guard: finding the Syncthing marker the
//! policy in `writ-core` is handed (ADR-028 §8).

use std::path::PathBuf;

use tempfile::TempDir;
use writ_core::startup::{classify_data_dir, DataDirVerdict, Platform, SyncProvider};
use writ_tauri_lib::startup::stfolder_markers;

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
