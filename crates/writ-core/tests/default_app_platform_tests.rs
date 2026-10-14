use std::collections::HashSet;
use std::path::{Path, PathBuf};

use writ_core::default_app::{
    aggregate_status, application_dirs, claimable_types, desktop_entry_status, find_desktop_entry,
    DefaultAppStatus, DESKTOP_ENTRY_NAMES, WINDOWS_DEFAULT_APPS_URI,
};

#[test]
fn every_group_names_the_mime_types_it_claims_on_linux() {
    for group in claimable_types() {
        assert!(!group.mimes.is_empty(), "{} claims no MIME type", group.id);
        assert!(
            group.mimes.iter().all(|mime| mime.contains('/')),
            "{} carries a malformed MIME type",
            group.id
        );
    }
}

#[test]
fn xdg_mime_answers_are_read_against_writs_own_entry() {
    assert_eq!(
        desktop_entry_status("Writ.desktop\n", "Writ.desktop"),
        DefaultAppStatus::IsDefault
    );
    assert_eq!(
        desktop_entry_status("\n", "Writ.desktop"),
        DefaultAppStatus::NoHandler
    );
    assert_eq!(
        desktop_entry_status("org.gnome.TextEditor.desktop\n", "Writ.desktop"),
        DefaultAppStatus::OtherApp {
            name: Some("org.gnome.TextEditor".to_string())
        }
    );
}

#[test]
fn application_folders_follow_the_xdg_order_and_defaults() {
    let home = Path::new("/home/a");
    assert_eq!(
        application_dirs(None, None, Some(home)),
        vec![
            PathBuf::from("/home/a/.local/share/applications"),
            PathBuf::from("/usr/local/share/applications"),
            PathBuf::from("/usr/share/applications"),
        ]
    );
    assert_eq!(
        application_dirs(Some("/data"), Some("/opt/share::/x"), Some(home)),
        vec![
            PathBuf::from("/data/applications"),
            PathBuf::from("/opt/share/applications"),
            PathBuf::from("/x/applications"),
        ]
    );
}

#[test]
fn the_desktop_entry_is_found_under_either_name() {
    let dirs = application_dirs(None, None, Some(Path::new("/home/a")));
    let present: HashSet<PathBuf> = [PathBuf::from("/usr/share/applications/writ.desktop")].into();
    assert_eq!(
        find_desktop_entry(&dirs, |path| present.contains(path)),
        Some("writ.desktop")
    );
    assert_eq!(find_desktop_entry(&dirs, |_| false), None);
    assert!(DESKTOP_ENTRY_NAMES.contains(&"Writ.desktop"));
}

#[test]
fn a_platform_that_picks_defaults_in_its_own_settings_says_so_for_the_group() {
    assert_eq!(
        aggregate_status(&[
            DefaultAppStatus::SystemSettings,
            DefaultAppStatus::SystemSettings
        ]),
        DefaultAppStatus::SystemSettings
    );
    assert!(!DefaultAppStatus::SystemSettings.is_unsupported());
    assert_eq!(WINDOWS_DEFAULT_APPS_URI, "ms-settings:defaultapps");
}

#[test]
fn the_system_settings_status_crosses_ipc_by_name() {
    assert_eq!(
        serde_json::to_string(&DefaultAppStatus::SystemSettings).expect("serialise"),
        r#"{"status":"system_settings"}"#
    );
}
