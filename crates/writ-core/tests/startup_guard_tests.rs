//! The data-folder verdict: which locations Writ will not keep its database
//! in, and what it tells the person who put it there (ADR-028 §8).
//!
//! Every case is exercised for all three platform tables from one host,
//! because [`classify_data_dir`] takes the platform as data.

use std::path::{Path, PathBuf};

use writ_core::startup::{
    classify_data_dir, data_dir_refusal_message, DataDirVerdict, Platform, StartupStage,
    SyncProvider,
};

const HOME: &str = "/home/u";

fn home() -> PathBuf {
    PathBuf::from(HOME)
}

fn classify(platform: Platform, data_dir: &str) -> DataDirVerdict {
    classify_data_dir(platform, Path::new(data_dir), Some(&home()), None, &[])
}

fn expect_provider(verdict: &DataDirVerdict, expected: SyncProvider, root: &str) {
    match verdict {
        DataDirVerdict::InsideSyncProvider {
            provider,
            root: got,
        } => {
            assert_eq!(*provider, expected);
            assert_eq!(got, Path::new(root));
        }
        other => panic!("expected a sync-provider verdict, got {other:?}"),
    }
}

#[test]
fn macos_icloud_mobile_documents_is_refused() {
    let verdict = classify(
        Platform::Macos,
        "/home/u/Library/Mobile Documents/writ/.writ",
    );
    expect_provider(
        &verdict,
        SyncProvider::ICloud,
        "/home/u/Library/Mobile Documents",
    );
}

#[test]
fn macos_cloudstorage_is_refused() {
    let verdict = classify(
        Platform::Macos,
        "/home/u/Library/CloudStorage/Dropbox/.writ",
    );
    expect_provider(
        &verdict,
        SyncProvider::Dropbox,
        "/home/u/Library/CloudStorage/Dropbox",
    );
}

#[test]
fn macos_cloudstorage_names_the_provider_in_the_container_folder() {
    let verdict = classify(
        Platform::Macos,
        "/home/u/Library/CloudStorage/GoogleDrive-u@example.com/My Drive/.writ",
    );
    expect_provider(
        &verdict,
        SyncProvider::GoogleDrive,
        "/home/u/Library/CloudStorage/GoogleDrive-u@example.com",
    );

    let verdict = classify(
        Platform::Macos,
        "/home/u/Library/CloudStorage/OneDrive-Personal/.writ",
    );
    expect_provider(
        &verdict,
        SyncProvider::OneDrive,
        "/home/u/Library/CloudStorage/OneDrive-Personal",
    );
}

#[test]
fn macos_dropbox_is_refused() {
    let verdict = classify(Platform::Macos, "/home/u/Dropbox/notes/.writ");
    expect_provider(&verdict, SyncProvider::Dropbox, "/home/u/Dropbox");
}

#[test]
fn macos_google_drive_is_refused() {
    let verdict = classify(Platform::Macos, "/home/u/Google Drive/.writ");
    expect_provider(&verdict, SyncProvider::GoogleDrive, "/home/u/Google Drive");
}

#[test]
fn windows_onedrive_is_refused() {
    let verdict = classify(Platform::Windows, "/home/u/OneDrive/Documents/.writ");
    expect_provider(&verdict, SyncProvider::OneDrive, "/home/u/OneDrive");
}

#[test]
fn windows_dropbox_is_refused() {
    let verdict = classify(Platform::Windows, "/home/u/Dropbox/.writ");
    expect_provider(&verdict, SyncProvider::Dropbox, "/home/u/Dropbox");
}

#[test]
fn windows_google_drive_is_refused() {
    let verdict = classify(Platform::Windows, "/home/u/Google Drive/.writ");
    expect_provider(&verdict, SyncProvider::GoogleDrive, "/home/u/Google Drive");
}

#[test]
fn linux_dropbox_is_refused() {
    let verdict = classify(Platform::Linux, "/home/u/Dropbox/.writ");
    expect_provider(&verdict, SyncProvider::Dropbox, "/home/u/Dropbox");
}

#[test]
fn linux_google_drive_is_refused() {
    let verdict = classify(Platform::Linux, "/home/u/Google Drive/.writ");
    expect_provider(&verdict, SyncProvider::GoogleDrive, "/home/u/Google Drive");
}

#[test]
fn linux_stfolder_marker_is_refused() {
    let markers = vec![PathBuf::from("/home/u/Sync")];
    let verdict = classify_data_dir(
        Platform::Linux,
        Path::new("/home/u/Sync/notes/.writ"),
        Some(&home()),
        None,
        &markers,
    );
    expect_provider(&verdict, SyncProvider::Syncthing, "/home/u/Sync");
}

#[test]
fn an_stfolder_marker_is_refused_on_every_platform() {
    let markers = vec![PathBuf::from("/home/u/Sync")];
    for platform in [Platform::Macos, Platform::Windows, Platform::Linux] {
        let verdict = classify_data_dir(
            platform,
            Path::new("/home/u/Sync/.writ"),
            Some(&home()),
            None,
            &markers,
        );
        expect_provider(&verdict, SyncProvider::Syncthing, "/home/u/Sync");
    }
}

#[test]
fn a_plain_home_writ_directory_is_ok() {
    for platform in [Platform::Macos, Platform::Windows, Platform::Linux] {
        assert_eq!(classify(platform, "/home/u/.writ"), DataDirVerdict::Ok);
    }
}

#[test]
fn a_folder_whose_name_merely_starts_with_a_provider_name_is_ok() {
    assert_eq!(
        classify(Platform::Macos, "/home/u/Dropbox-archive/.writ"),
        DataDirVerdict::Ok
    );
}

#[test]
fn each_platform_checks_only_its_own_table() {
    assert_eq!(
        classify(Platform::Linux, "/home/u/Library/Mobile Documents/.writ"),
        DataDirVerdict::Ok
    );
    assert_eq!(
        classify(Platform::Linux, "/home/u/OneDrive/.writ"),
        DataDirVerdict::Ok
    );
    assert_eq!(
        classify(Platform::Macos, "/home/u/OneDrive/.writ"),
        DataDirVerdict::Ok
    );
}

#[test]
fn a_data_directory_without_a_home_is_ok() {
    assert_eq!(
        classify_data_dir(
            Platform::Macos,
            Path::new("/opt/writ/.writ"),
            None,
            None,
            &[]
        ),
        DataDirVerdict::Ok
    );
}

#[test]
fn a_data_directory_inside_the_notes_folder_is_refused() {
    let notes = PathBuf::from("/home/u/Writ");
    let verdict = classify_data_dir(
        Platform::Linux,
        Path::new("/home/u/Writ/.writ"),
        Some(&home()),
        Some(&notes),
        &[],
    );
    assert_eq!(
        verdict,
        DataDirVerdict::InsideNotesFolder { notes_root: notes }
    );
}

#[test]
fn a_notes_folder_inside_the_data_directory_is_refused() {
    let notes = PathBuf::from("/home/u/.writ/notes");
    let verdict = classify_data_dir(
        Platform::Linux,
        Path::new("/home/u/.writ"),
        Some(&home()),
        Some(&notes),
        &[],
    );
    assert_eq!(
        verdict,
        DataDirVerdict::InsideNotesFolder { notes_root: notes }
    );
}

/// `resolve_notes_root_from` puts the notes folder at `<data dir>/Writ`
/// whenever a data-folder override is in force, so a dev, recording or smoke
/// instance depends on this exemption to start at all.
#[test]
fn the_default_notes_folder_inside_the_data_directory_is_ok() {
    let notes = PathBuf::from("/home/u/.writ-dev/Writ");
    let verdict = classify_data_dir(
        Platform::Linux,
        Path::new("/home/u/.writ-dev"),
        Some(&home()),
        Some(&notes),
        &[],
    );
    assert_eq!(verdict, DataDirVerdict::Ok);
}

#[test]
fn a_notes_folder_beside_the_data_directory_is_ok() {
    let notes = PathBuf::from("/home/u/Writ");
    let verdict = classify_data_dir(
        Platform::Linux,
        Path::new("/home/u/.writ"),
        Some(&home()),
        Some(&notes),
        &[],
    );
    assert_eq!(verdict, DataDirVerdict::Ok);
}

#[test]
fn the_refusal_message_names_the_folder_and_the_provider() {
    let message = data_dir_refusal_message(&DataDirVerdict::InsideSyncProvider {
        provider: SyncProvider::Dropbox,
        root: PathBuf::from("/home/u/Dropbox"),
    });
    assert!(message.contains("Dropbox"));
    assert!(message.contains("/home/u/Dropbox"));
    assert!(message.contains("WRIT_DATA_DIR"));
}

#[test]
fn the_notes_refusal_message_names_the_notes_folder() {
    let message = data_dir_refusal_message(&DataDirVerdict::InsideNotesFolder {
        notes_root: PathBuf::from("/home/u/Writ"),
    });
    assert!(message.contains("/home/u/Writ"));
    assert!(message.contains("WRIT_DATA_DIR"));
}

#[test]
fn an_ok_verdict_has_no_message() {
    assert!(data_dir_refusal_message(&DataDirVerdict::Ok).is_empty());
}

/// ADR-028 §10, the same list the two scanners carry. A third copy because
/// this crate cannot import a test module from `src-tauri`.
const BANNED: &[&str] = &[
    "vault",
    "buffer",
    "scratchpad",
    "second brain",
    "render surface",
    "inbox",
    "reveal",
    "threshold",
    "refuse",
    "debounce",
    "source",
    "dialect",
    "FTS",
    "IPC",
    "sidecar",
    "MiB",
    "syntax highlighting",
    "typography",
];

#[test]
fn the_refusal_message_contains_no_banned_words() {
    let providers = [
        SyncProvider::ICloud,
        SyncProvider::Dropbox,
        SyncProvider::GoogleDrive,
        SyncProvider::OneDrive,
        SyncProvider::Syncthing,
    ];
    let mut messages: Vec<String> = providers
        .iter()
        .map(|provider| {
            data_dir_refusal_message(&DataDirVerdict::InsideSyncProvider {
                provider: *provider,
                root: PathBuf::from("/home/u/Sync"),
            })
        })
        .collect();
    messages.push(data_dir_refusal_message(
        &DataDirVerdict::InsideNotesFolder {
            notes_root: PathBuf::from("/home/u/Writ"),
        },
    ));
    for name in [
        "Box",
        "pCloud",
        "CloudStorage",
        "Apple's cloud storage service",
    ] {
        messages.push(data_dir_refusal_message(
            &DataDirVerdict::InsideSyncContainer {
                name: name.to_string(),
                root: PathBuf::from("/home/u/Library/CloudStorage").join(name),
            },
        ));
    }
    messages.extend(
        providers
            .iter()
            .map(|provider| provider.label().to_string()),
    );
    messages.push(StartupStage::DataDirectoryLocation.describe().to_string());
    messages.push(StartupStage::DataDirectoryLocation.remedy().to_string());

    for message in &messages {
        let haystack = message.to_lowercase();
        for word in BANNED {
            assert!(
                !haystack.contains(&word.to_lowercase()),
                "{message:?} says {word:?}"
            );
        }
    }
}

#[test]
fn the_location_stage_describes_the_check_and_names_the_environment_variable() {
    assert_eq!(
        StartupStage::DataDirectoryLocation.describe(),
        "checking where Writ keeps its data"
    );
    assert!(StartupStage::DataDirectoryLocation
        .remedy()
        .contains("WRIT_DATA_DIR"));
}

/// APFS and NTFS are case-preserving but case-insensitive, so `~/dropbox` and
/// `~/Dropbox` are one folder there and the guard has to see both spellings.
/// The path is judged as data, so no case variant of it needs to exist on
/// disk for this to hold.
#[test]
fn a_lowercase_provider_folder_is_refused_on_the_case_insensitive_platforms() {
    for (platform, data_dir, provider, root) in [
        (
            Platform::Macos,
            "/home/u/dropbox/newdata",
            SyncProvider::Dropbox,
            "/home/u/dropbox",
        ),
        (
            Platform::Macos,
            "/home/u/GOOGLE DRIVE/.writ",
            SyncProvider::GoogleDrive,
            "/home/u/GOOGLE DRIVE",
        ),
        (
            Platform::Macos,
            "/home/u/library/mobile documents/.writ",
            SyncProvider::ICloud,
            "/home/u/library/mobile documents",
        ),
        (
            Platform::Windows,
            "/home/u/onedrive/.writ",
            SyncProvider::OneDrive,
            "/home/u/onedrive",
        ),
        (
            Platform::Windows,
            "/home/u/DropBox/.writ",
            SyncProvider::Dropbox,
            "/home/u/DropBox",
        ),
    ] {
        expect_provider(&classify(platform, data_dir), provider, root);
    }
}

/// The same spellings on Linux, where `~/dropbox` really is a different
/// folder from `~/Dropbox` and refusing it would be wrong.
#[test]
fn a_lowercase_provider_folder_is_ok_on_linux() {
    for data_dir in [
        "/home/u/dropbox/newdata",
        "/home/u/google drive/.writ",
        "/home/u/DROPBOX/.writ",
    ] {
        assert_eq!(classify(Platform::Linux, data_dir), DataDirVerdict::Ok);
    }
}

/// The container name is what says which service owns a `Library/CloudStorage`
/// folder, so a case difference there must not fall back to the table's
/// default and send the user to the wrong service.
#[test]
fn macos_cloudstorage_names_the_provider_in_a_lowercase_container() {
    for (data_dir, provider, root) in [
        (
            "/home/u/Library/CloudStorage/dropbox/.writ",
            SyncProvider::Dropbox,
            "/home/u/Library/CloudStorage/dropbox",
        ),
        (
            "/home/u/library/cloudstorage/googledrive-me@example.com/.writ",
            SyncProvider::GoogleDrive,
            "/home/u/library/cloudstorage/googledrive-me@example.com",
        ),
        (
            "/home/u/Library/CloudStorage/onedrive-Personal/.writ",
            SyncProvider::OneDrive,
            "/home/u/Library/CloudStorage/onedrive-Personal",
        ),
    ] {
        expect_provider(&classify(Platform::Macos, data_dir), provider, root);
    }
}

/// The exemption has to fold case for the same reason the provider table
/// does: on macOS `<data dir>/writ` is the folder `resolve_notes_root_from`
/// created as `<data dir>/Writ`, and refusing it would stop every instance
/// running against its own data folder.
#[test]
fn the_default_notes_folder_inside_the_data_directory_is_ok_whatever_its_case() {
    let notes = PathBuf::from("/home/u/.writ-dev/writ");
    let verdict = classify_data_dir(
        Platform::Macos,
        Path::new("/home/u/.writ-dev"),
        Some(&home()),
        Some(&notes),
        &[],
    );
    assert_eq!(verdict, DataDirVerdict::Ok);
}

/// `Library/CloudStorage` is Apple's File Provider area, not iCloud Drive's
/// folder: every vendor that ships a File Provider extension gets a container
/// there. A container Writ has no variant for is still refused, and the
/// refusal names the container rather than a service the user is not running.
#[test]
fn macos_cloudstorage_names_the_container_when_the_service_is_not_one_writ_knows() {
    for (data_dir, name) in [
        ("/home/u/Library/CloudStorage/Dropbox/.writ", "Dropbox"),
        ("/home/u/Library/CloudStorage/Box/.writ", "Box"),
        (
            "/home/u/Library/CloudStorage/Box-me@example.com/.writ",
            "Box",
        ),
        (
            "/home/u/Library/CloudStorage/pCloud-Personal/.writ",
            "pCloud",
        ),
        ("/home/u/Library/Mobile Documents/.writ", "iCloud Drive"),
        ("/home/u/Library/CloudStorage/box/.writ", "Box"),
        (
            "/home/u/Library/CloudStorage/box-me@example.com/.writ",
            "Box",
        ),
        (
            "/home/u/Library/CloudStorage/.writ",
            "Apple's cloud storage service",
        ),
    ] {
        let message = data_dir_refusal_message(&classify(Platform::Macos, data_dir));
        assert!(
            message.contains(&format!("which {name} syncs")),
            "{data_dir} should be refused by name, got {message:?}"
        );
        assert!(message.contains(&format!("outside {name},")), "{message:?}");
    }
}
