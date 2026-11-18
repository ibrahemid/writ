//! A client entry made before the rename reads as connected while the path it
//! launches still reaches a build of the command (ADR-059 decision 7).

use std::ffi::OsString;
use std::path::Path;

use jotra_core::cli_install::CLI_SIGNATURE;
use jotra_core::mcp_clients::RegistrationStatus;
use jotra_storage::mcp_registration::{
    command_carries_signature, read_registration_status, resolve_command,
};

fn write_command(path: &Path) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
    let mut bytes = b"\x7fELF header ".to_vec();
    bytes.extend_from_slice(CLI_SIGNATURE);
    bytes.extend_from_slice(b" trailer");
    std::fs::write(path, bytes).expect("command");
}

fn write_other(path: &Path) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
    std::fs::write(path, b"#!/bin/sh\necho another program\n").expect("other");
}

fn search_path(folders: &[&Path]) -> OsString {
    std::env::join_paths(folders).expect("search path")
}

#[test]
fn a_signed_copy_carries_the_signature_and_another_file_does_not() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ours = dir.path().join("jotra");
    let other = dir.path().join("other");
    write_command(&ours);
    write_other(&other);

    assert!(command_carries_signature(&ours));
    assert!(!command_carries_signature(&other));
    assert!(!command_carries_signature(&dir.path().join("missing")));
}

#[test]
fn an_entry_through_the_old_install_folder_reads_connected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let command = dir
        .path()
        .join("Program Files")
        .join("Writ")
        .join("writ.exe");
    write_command(&command);

    for name in ["writ", "jotra"] {
        assert_eq!(
            read_registration_status(name, &command, None, true),
            RegistrationStatus::Connected,
            "{name}"
        );
    }
}

#[cfg(unix)]
#[test]
fn an_entry_through_the_old_app_link_reads_connected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let applications = dir.path().join("Applications");
    let bundle = applications.join("Jotra.app");
    write_command(&bundle.join("Contents").join("MacOS").join("writ"));
    std::os::unix::fs::symlink(&bundle, applications.join("Writ.app")).expect("app link");
    let command = applications
        .join("Writ.app")
        .join("Contents")
        .join("MacOS")
        .join("writ");

    assert_eq!(
        read_registration_status("writ", &command, None, false),
        RegistrationStatus::Connected
    );
}

#[cfg(unix)]
#[test]
fn a_command_link_into_the_old_app_link_reads_connected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let applications = dir.path().join("Applications");
    let bundle = applications.join("Jotra.app");
    write_command(&bundle.join("Contents").join("MacOS").join("writ"));
    std::os::unix::fs::symlink(&bundle, applications.join("Writ.app")).expect("app link");
    let bin = dir.path().join("usr").join("local").join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let command = bin.join("writ");
    std::os::unix::fs::symlink(
        applications
            .join("Writ.app")
            .join("Contents")
            .join("MacOS")
            .join("writ"),
        &command,
    )
    .expect("command link");

    assert_eq!(
        read_registration_status("writ", &command, None, false),
        RegistrationStatus::Connected
    );
}

#[test]
fn a_bare_command_name_is_found_on_the_search_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let empty = dir.path().join("empty");
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&empty).expect("empty");
    write_command(&bin.join("writ"));
    let path = search_path(&[&empty, &bin]);

    assert_eq!(
        resolve_command(Path::new("writ"), Some(&path), false),
        Some(bin.join("writ"))
    );
    assert_eq!(
        read_registration_status("writ", Path::new("writ"), Some(&path), false),
        RegistrationStatus::Connected
    );
    assert_eq!(resolve_command(Path::new("writ"), None, false), None);
    assert_eq!(resolve_command(Path::new(""), Some(&path), false), None);
}

#[test]
fn a_bare_command_name_finds_the_windows_suffix() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = dir.path().join("bin");
    write_command(&bin.join("writ.exe"));
    let path = search_path(&[&bin]);

    assert_eq!(
        resolve_command(Path::new("writ"), Some(&path), true),
        Some(bin.join("writ.exe"))
    );
    assert_eq!(resolve_command(Path::new("writ"), Some(&path), false), None);
}

#[test]
fn a_command_that_moved_away_or_is_another_program_reads_broken() {
    let dir = tempfile::tempdir().expect("tempdir");
    let other = dir.path().join("bin").join("writ");
    write_other(&other);

    assert_eq!(
        read_registration_status("writ", &dir.path().join("gone").join("writ"), None, false),
        RegistrationStatus::Broken
    );
    assert_eq!(
        read_registration_status("writ", &other, None, false),
        RegistrationStatus::Broken
    );
}

#[cfg(unix)]
#[test]
fn a_link_whose_target_was_deleted_reads_broken() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir
        .path()
        .join("Writ.app")
        .join("Contents")
        .join("MacOS")
        .join("writ");
    write_command(&target);
    let command = dir.path().join("writ");
    std::os::unix::fs::symlink(&target, &command).expect("link");
    std::fs::remove_file(&target).expect("delete target");

    assert_eq!(
        read_registration_status("writ", &command, None, false),
        RegistrationStatus::Broken
    );
}

#[test]
fn an_entry_under_another_name_is_unrelated() {
    let dir = tempfile::tempdir().expect("tempdir");
    let command = dir.path().join("writ");
    write_command(&command);

    assert_eq!(
        read_registration_status("github", &command, None, false),
        RegistrationStatus::Unrelated
    );
}
