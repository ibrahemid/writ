//! The command finds the app beside it on an install whose app binary still
//! carries the name from before the rename (ADR-059 decision 12).

use std::path::Path;

use jotra_cli::{legacy_gui_binary_name, resolve_gui_binary, GuiLaunch, GUI_BINARY_NAME};

fn touch(path: &Path) {
    std::fs::write(path, b"").expect("touch");
}

#[test]
fn the_legacy_app_binary_name_is_the_old_stem_with_the_platform_suffix() {
    assert_eq!(
        legacy_gui_binary_name(),
        format!("writ-tauri{}", std::env::consts::EXE_SUFFIX)
    );
}

#[test]
fn a_sibling_under_the_old_name_is_launched_when_the_new_one_is_absent() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let legacy = dir.path().join(legacy_gui_binary_name());
    touch(&legacy);
    let cli = dir.path().join("writ");
    touch(&cli);

    assert_eq!(
        resolve_gui_binary(None, Some(&cli)),
        GuiLaunch::Binary(legacy)
    );
}

#[test]
fn the_new_sibling_wins_over_the_old_one() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let current = dir.path().join(GUI_BINARY_NAME);
    touch(&current);
    touch(&dir.path().join(legacy_gui_binary_name()));
    let cli = dir.path().join("jotra");
    touch(&cli);

    assert_eq!(
        resolve_gui_binary(None, Some(&cli)),
        GuiLaunch::Binary(current)
    );
}

#[test]
fn no_sibling_under_either_name_falls_to_the_desktop_handler() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let cli = dir.path().join("writ");
    touch(&cli);

    assert_eq!(resolve_gui_binary(None, Some(&cli)), GuiLaunch::OsDefault);
}
