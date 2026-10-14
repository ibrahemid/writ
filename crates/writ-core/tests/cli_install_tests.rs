use std::path::{Path, PathBuf};

use writ_core::cli_install::{
    carries_cli_signature, client_command, install_target, is_in_appimage_mount, is_translocated,
    is_writ_command_target, may_replace, ClientCommandFacts, InstallFacts, InstallTarget,
    InstalledEntry, CLI_SIGNATURE, SYSTEM_LINK,
};

fn facts(windows: bool, in_appimage: bool, home: Option<&Path>) -> InstallFacts<'_> {
    InstallFacts {
        windows,
        in_appimage,
        home,
    }
}

#[test]
fn macos_and_linux_packages_link_into_usr_local_bin() {
    assert_eq!(
        install_target(facts(false, false, Some(Path::new("/home/a")))),
        Some(InstallTarget::Link(PathBuf::from(SYSTEM_LINK)))
    );
}

#[test]
fn an_appimage_copies_into_the_users_own_bin_folder() {
    assert_eq!(
        install_target(facts(false, true, Some(Path::new("/home/a")))),
        Some(InstallTarget::Copy(PathBuf::from(
            "/home/a/.local/bin/writ"
        )))
    );
    assert_eq!(install_target(facts(false, true, None)), None);
}

#[test]
fn windows_leaves_the_path_to_the_installer() {
    assert_eq!(
        install_target(facts(true, false, Some(Path::new("C:/Users/a")))),
        Some(InstallTarget::ByInstaller)
    );
}

#[test]
fn a_translocated_app_is_recognised_by_its_folder() {
    assert!(is_translocated(Path::new(
        "/private/var/folders/x/T/AppTranslocation/1A2B/d/Writ.app/Contents/MacOS/writ-tauri"
    )));
    assert!(!is_translocated(Path::new(
        "/Applications/Writ.app/Contents/MacOS/writ-tauri"
    )));
    assert!(!is_translocated(Path::new(
        "/Users/a/AppTranslocationNotes/writ"
    )));
}

#[test]
fn an_appimage_mount_is_recognised_by_its_folder() {
    assert!(is_in_appimage_mount(Path::new(
        "/tmp/.mount_WritAbC123/usr/bin/writ"
    )));
    assert!(!is_in_appimage_mount(Path::new("/usr/bin/writ")));
}

#[test]
fn a_link_into_an_app_bundle_is_writs_own() {
    assert!(is_writ_command_target(
        Path::new("/Applications/Writ.app/Contents/MacOS/writ"),
        false
    ));
    // A bundle that was deleted or renamed since still names the command.
    assert!(is_writ_command_target(
        Path::new("/Users/a/Downloads/Writ 2.app/Contents/MacOS/writ"),
        false
    ));
    assert!(is_writ_command_target(
        Path::new("/tmp/.mount_WritAbC/usr/bin/writ"),
        false
    ));
    assert!(is_writ_command_target(Path::new("/usr/bin/writ"), true));
}

#[test]
fn a_link_anywhere_else_belongs_to_something_else() {
    assert!(!is_writ_command_target(
        Path::new("/opt/homebrew/bin/writ"),
        false
    ));
    assert!(!is_writ_command_target(
        Path::new("/Applications/Other.app/Contents/MacOS/other"),
        false
    ));
    assert!(!is_writ_command_target(
        Path::new("/Applications/Writ.app/Contents/Resources/writ"),
        false
    ));
    assert!(!is_writ_command_target(Path::new("/usr/bin/writer"), true));
}

#[test]
fn only_writs_own_entry_or_nothing_may_be_replaced() {
    assert!(may_replace(&InstalledEntry::Absent));
    assert!(may_replace(&InstalledEntry::Link {
        target: PathBuf::from("/Applications/Writ.app/Contents/MacOS/writ"),
        beside_app: false,
    }));
    assert!(!may_replace(&InstalledEntry::Link {
        target: PathBuf::from("/opt/tools/writ"),
        beside_app: false,
    }));
    assert!(may_replace(&InstalledEntry::File {
        is_writ_command: true
    }));
    assert!(!may_replace(&InstalledEntry::File {
        is_writ_command: false
    }));
    assert!(!may_replace(&InstalledEntry::Other));
}

#[test]
fn the_signature_is_found_anywhere_in_a_binary() {
    let mut bytes = vec![0x7f, b'E', b'L', b'F', 0, 0];
    bytes.extend_from_slice(CLI_SIGNATURE);
    bytes.extend_from_slice(&[0; 16]);
    assert!(carries_cli_signature(&bytes));
    assert!(!carries_cli_signature(
        b"#!/bin/sh\nexec other-writ \"$@\"\n"
    ));
    assert!(!carries_cli_signature(b""));
}

#[test]
fn an_appimage_hands_clients_the_copy_it_installs() {
    let target = InstallTarget::Copy(PathBuf::from("/home/a/.local/bin/writ"));
    let bundled = Path::new("/tmp/.mount_WritX/usr/bin/writ");
    let before = client_command(ClientCommandFacts {
        exe: Some(Path::new("/tmp/.mount_WritX/usr/bin/writ-tauri")),
        bundled: Some(bundled),
        target: Some(&target),
        installed: false,
    });
    assert_eq!(before.path, PathBuf::from("/home/a/.local/bin/writ"));
    assert!(before.needs_install);
    assert!(!before.needs_move);

    let after = client_command(ClientCommandFacts {
        exe: None,
        bundled: Some(bundled),
        target: Some(&target),
        installed: true,
    });
    assert!(!after.needs_install);
}

#[test]
fn an_installed_app_hands_clients_its_bundled_command() {
    let target = InstallTarget::Link(PathBuf::from(SYSTEM_LINK));
    let command = client_command(ClientCommandFacts {
        exe: Some(Path::new(
            "/Applications/Writ.app/Contents/MacOS/writ-tauri",
        )),
        bundled: Some(Path::new("/Applications/Writ.app/Contents/MacOS/writ")),
        target: Some(&target),
        installed: false,
    });
    assert_eq!(
        command.path,
        PathBuf::from("/Applications/Writ.app/Contents/MacOS/writ")
    );
    assert!(!command.needs_install);
    assert!(!command.needs_move);
}

#[test]
fn a_translocated_app_asks_to_be_moved_first() {
    let target = InstallTarget::Link(PathBuf::from(SYSTEM_LINK));
    let command = client_command(ClientCommandFacts {
        exe: Some(Path::new(
            "/private/var/folders/x/T/AppTranslocation/1A/d/Writ.app/Contents/MacOS/writ-tauri",
        )),
        bundled: Some(Path::new(
            "/private/var/folders/x/T/AppTranslocation/1A/d/Writ.app/Contents/MacOS/writ",
        )),
        target: Some(&target),
        installed: false,
    });
    assert!(command.needs_move);
}

#[test]
fn a_build_with_nothing_bundled_names_the_install_target() {
    let link = InstallTarget::Link(PathBuf::from(SYSTEM_LINK));
    let unix = client_command(ClientCommandFacts {
        exe: None,
        bundled: None,
        target: Some(&link),
        installed: false,
    });
    assert_eq!(unix.path, PathBuf::from(SYSTEM_LINK));

    let windows = client_command(ClientCommandFacts {
        exe: None,
        bundled: None,
        target: Some(&InstallTarget::ByInstaller),
        installed: false,
    });
    assert_eq!(windows.path, PathBuf::from("writ.exe"));
}
