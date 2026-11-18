//! Registrations, scripts and extensions made before the rename keep working
//! against the renamed command and server (ADR-059 decision 7).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use jotra_core::cli_install::{
    is_command_file_name, is_jotra_command_target, legacy_command_path, may_replace,
    should_refresh_legacy_command, InstallTarget, InstalledEntry,
};
use jotra_core::mcp_clients::{
    bundle_launcher, bundle_manifest, is_probe_client, is_server_name, registration_status,
    repair_command, RegistrationStatus,
};
use jotra_core::mcp_dialect::{is_open_in_app_tool, select_dialect, Dialect, COMMAND_NAME_VAR};

const APP: &str = "/Applications/Jotra.app/Contents/MacOS/jotra";

fn dialect(command_name: Option<&str>, argv0: Option<&str>) -> Dialect {
    select_dialect(command_name.map(OsStr::new), argv0.map(OsStr::new))
}

#[test]
fn the_variable_a_wrapper_exports_selects_the_dialect() {
    assert_eq!(COMMAND_NAME_VAR, "JOTRA_COMMAND_NAME");
    assert_eq!(
        dialect(Some("writ"), Some("/usr/lib/jotra/jotra-cli")),
        Dialect::Legacy
    );
    assert_eq!(
        dialect(Some("jotra"), Some("/usr/local/bin/writ")),
        Dialect::Current,
        "the variable decides over the file the binary was started as"
    );
    assert_eq!(
        dialect(Some("writ-cli"), Some("/opt/jotra")),
        Dialect::Legacy
    );
}

#[test]
fn a_blank_variable_leaves_the_choice_to_the_started_name() {
    assert_eq!(
        dialect(Some(""), Some("/usr/local/bin/writ")),
        Dialect::Legacy
    );
    assert_eq!(
        dialect(Some("  "), Some("/usr/local/bin/jotra")),
        Dialect::Current
    );
}

#[test]
fn both_legacy_stems_select_the_legacy_dialect() {
    for argv0 in [
        "writ",
        "/usr/local/bin/writ",
        "/Applications/Writ.app/Contents/MacOS/writ",
        "C:\\Program Files\\Writ\\writ.exe",
        "C:/Program Files/Writ/WRIT.EXE",
        "writ-cli",
        "/home/u/.local/share/writ/writ-cli",
    ] {
        assert_eq!(dialect(None, Some(argv0)), Dialect::Legacy, "{argv0}");
    }
}

#[test]
fn any_other_stem_selects_the_current_dialect() {
    for argv0 in [
        "jotra",
        "/usr/local/bin/jotra",
        "C:/Program Files/Jotra/jotra.exe",
        "jotra-cli",
        "writer",
        "my-writ",
        "",
    ] {
        assert_eq!(dialect(None, Some(argv0)), Dialect::Current, "{argv0}");
    }
    assert_eq!(dialect(None, None), Dialect::Current);
}

#[test]
fn each_dialect_lists_its_own_names() {
    assert_eq!(Dialect::Legacy.server_name(), "writ");
    assert_eq!(Dialect::Legacy.listed_open_tool(), "open_in_writ");
    assert_eq!(Dialect::Legacy.unlisted_open_tool(), "open_in_jotra");
    assert_eq!(Dialect::Legacy.resource_scheme(), "writ-folder");

    assert_eq!(Dialect::Current.server_name(), "jotra");
    assert_eq!(Dialect::Current.listed_open_tool(), "open_in_jotra");
    assert_eq!(Dialect::Current.unlisted_open_tool(), "open_in_writ");
    assert_eq!(Dialect::Current.resource_scheme(), "jotra-folder");
}

#[test]
fn both_tool_names_show_a_file_in_the_app() {
    for dialect in Dialect::ALL {
        assert!(is_open_in_app_tool(dialect.listed_open_tool()));
        assert!(is_open_in_app_tool(dialect.unlisted_open_tool()));
    }
    assert!(!is_open_in_app_tool("open_in_writer"));
    assert!(!is_open_in_app_tool("read_note"));
}

#[test]
fn the_bundle_manifest_keeps_the_extension_name_and_runs_the_legacy_command() {
    let manifest: serde_json::Value =
        serde_json::from_str(&bundle_manifest(APP, "0.7.0")).expect("json");

    assert_eq!(
        manifest,
        serde_json::json!({
            "manifest_version": "0.3",
            "name": "writ",
            "display_name": "Jotra",
            "version": "0.7.0",
            "description": "Reads the files in your Jotra folder, and writes them if you allow it in Jotra.",
            "author": { "name": "Jotra" },
            "homepage": "https://writ.ibrahemid.com",
            "server": {
                "type": "node",
                "entry_point": "server/index.js",
                "mcp_config": {
                    "command": "node",
                    "args": ["${__dirname}/server/index.js"],
                    "env": {
                        "JOTRA_COMMAND": APP,
                        "JOTRA_COMMAND_NAME": "writ",
                    },
                },
            },
            "compatibility": {
                "platforms": ["darwin", "win32", "linux"],
            },
        })
    );
}

#[test]
fn the_launcher_reads_the_new_variable_then_the_old_one() {
    let launcher = bundle_launcher();
    assert!(
        launcher.contains(
            "const command = process.env.JOTRA_COMMAND || process.env[\"WRIT_COMMAND\"];"
        ),
        "{launcher}"
    );
}

#[test]
fn both_probe_names_are_probes() {
    assert!(is_probe_client("jotra-connection-test"));
    assert!(is_probe_client("writ-connection-test"));
    assert!(!is_probe_client("writ"));
    assert!(!is_probe_client("Claude Desktop"));
}

#[test]
fn either_server_name_is_the_servers_entry() {
    assert!(is_server_name("jotra"));
    assert!(is_server_name("writ"));
    assert!(!is_server_name("writer"));
    assert!(!is_server_name("Writ"));
}

#[test]
fn an_entry_under_either_name_is_connected_when_its_command_is_ours() {
    for name in ["writ", "jotra"] {
        assert_eq!(
            registration_status(name, true),
            RegistrationStatus::Connected,
            "{name}"
        );
        assert_eq!(
            registration_status(name, false),
            RegistrationStatus::Broken,
            "{name}"
        );
    }
    assert_eq!(
        registration_status("github", true),
        RegistrationStatus::Unrelated
    );
}

#[test]
fn a_repair_keeps_the_entry_name_and_runs_the_command_named_after_it() {
    assert_eq!(
        repair_command("writ", Path::new("/usr/local/bin/jotra")),
        Some(PathBuf::from("/usr/local/bin/writ"))
    );
    assert_eq!(
        repair_command("jotra", Path::new("/usr/local/bin/jotra")),
        Some(PathBuf::from("/usr/local/bin/jotra"))
    );
    assert_eq!(
        repair_command("writ", Path::new("/home/u/.local/bin/jotra")),
        Some(PathBuf::from("/home/u/.local/bin/writ"))
    );
    assert_eq!(
        repair_command("writ", Path::new("C:/Program Files/Jotra/jotra.exe")),
        Some(PathBuf::from("C:/Program Files/Jotra/writ.exe"))
    );
    assert_eq!(repair_command("github", Path::new(APP)), None);
}

#[test]
fn the_install_accepts_both_command_names() {
    for name in ["jotra", "jotra.exe", "writ", "writ.exe"] {
        assert!(is_command_file_name(OsStr::new(name)), "{name}");
    }
    for name in ["writer", "writ.sh", "jotra-tauri", "Writ"] {
        assert!(!is_command_file_name(OsStr::new(name)), "{name}");
    }
    assert!(is_jotra_command_target(
        Path::new("/Applications/Writ.app/Contents/MacOS/writ"),
        false
    ));
    assert!(is_jotra_command_target(
        Path::new("/Applications/Jotra.app/Contents/MacOS/writ"),
        false
    ));
    assert!(is_jotra_command_target(Path::new("/usr/bin/writ"), true));
    assert!(!is_jotra_command_target(Path::new("/usr/bin/writ"), false));
    assert!(may_replace(&InstalledEntry::Link {
        target: PathBuf::from("/Applications/Writ.app/Contents/MacOS/writ"),
        beside_app: false,
    }));
}

#[test]
fn the_legacy_name_sits_beside_the_installed_command() {
    assert_eq!(
        legacy_command_path(&InstallTarget::Link(PathBuf::from("/usr/local/bin/jotra"))),
        Some(PathBuf::from("/usr/local/bin/writ"))
    );
    assert_eq!(
        legacy_command_path(&InstallTarget::Copy(PathBuf::from(
            "/home/u/.local/bin/jotra"
        ))),
        Some(PathBuf::from("/home/u/.local/bin/writ"))
    );
    assert_eq!(legacy_command_path(&InstallTarget::ByInstaller), None);
}

#[test]
fn an_install_keeps_only_an_existing_legacy_name_of_ours_current() {
    assert!(!should_refresh_legacy_command(&InstalledEntry::Absent));
    assert!(should_refresh_legacy_command(&InstalledEntry::Link {
        target: PathBuf::from("/Applications/Writ.app/Contents/MacOS/writ"),
        beside_app: false,
    }));
    assert!(should_refresh_legacy_command(&InstalledEntry::File {
        is_jotra_command: true
    }));
    assert!(!should_refresh_legacy_command(&InstalledEntry::File {
        is_jotra_command: false
    }));
    assert!(!should_refresh_legacy_command(&InstalledEntry::Link {
        target: PathBuf::from("/opt/other/bin/writ"),
        beside_app: false,
    }));
    assert!(!should_refresh_legacy_command(&InstalledEntry::Other));
}
