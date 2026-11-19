use std::collections::HashSet;
use std::path::{Path, PathBuf};

use jotra_core::default_app::{
    application_dirs, desktop_entry_status, find_desktop_entry, DefaultAppStatus,
    DESKTOP_ENTRY_NAMES,
};
use jotra_core::legacy_names;

#[test]
fn the_entry_names_from_before_the_rename_are_listed_after_the_current_ones() {
    assert_eq!(
        DESKTOP_ENTRY_NAMES,
        [
            "Jotra.desktop",
            "jotra.desktop",
            "Writ.desktop",
            "writ.desktop"
        ]
    );
    assert_eq!(legacy_names::DESKTOP_ENTRY, "Writ.desktop");
    assert_eq!(legacy_names::LOWERCASE_DESKTOP_ENTRY, "writ.desktop");
}

#[test]
fn a_default_names_the_current_entry_when_both_ship() {
    let dirs = application_dirs(None, None, Some(Path::new("/home/a")));
    let present: HashSet<PathBuf> = [
        PathBuf::from("/home/a/.local/share/applications/Writ.desktop"),
        PathBuf::from("/usr/share/applications/Writ.desktop"),
        PathBuf::from("/usr/share/applications/Jotra.desktop"),
    ]
    .into();

    assert_eq!(
        find_desktop_entry(&dirs, |path| present.contains(path)),
        Some("Jotra.desktop")
    );
}

#[test]
fn an_install_with_only_an_old_entry_keeps_its_id() {
    let dirs = application_dirs(None, None, Some(Path::new("/home/a")));
    let present: HashSet<PathBuf> = [PathBuf::from(
        "/home/a/.local/share/applications/writ.desktop",
    )]
    .into();

    assert_eq!(
        find_desktop_entry(&dirs, |path| present.contains(path)),
        Some("writ.desktop")
    );
}

#[test]
fn every_one_of_the_four_ids_counts_as_jotra() {
    for ours in ["Jotra.desktop", "writ.desktop"] {
        for answer in DESKTOP_ENTRY_NAMES {
            assert_eq!(
                desktop_entry_status(&format!("{answer}\n"), ours),
                DefaultAppStatus::IsDefault,
                "{answer} against {ours}"
            );
        }
    }
}

#[test]
fn a_look_alike_entry_is_another_app() {
    assert_eq!(
        desktop_entry_status("writer.desktop", "Jotra.desktop"),
        DefaultAppStatus::OtherApp {
            name: Some("writer".to_string())
        }
    );
    assert_eq!(
        desktop_entry_status("WRIT.desktop", "Jotra.desktop"),
        DefaultAppStatus::OtherApp {
            name: Some("WRIT".to_string())
        }
    );
}
