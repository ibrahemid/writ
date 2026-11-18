use std::path::{Path, PathBuf};

use jotra_core::cli_install::CLI_SIGNATURE;
use jotra_core::linux_rename::{
    appimage_command_copies, autostart_file_name, desktop_entry_value, exec_argument,
    exec_with_app_path, is_stale_command_copy, plan_appimage_entry, plan_autostart,
    set_desktop_entry_value, user_applications_dir, AutostartFacts, AutostartPlan, CommandCopy,
    APPIMAGE_ENTRY_FILE, AUTOSTART_FOLDER, CLI_BINARY_STEM,
};

const PLUGIN_ENTRY: &str = "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Writ\nComment=Writ startup script\nExec=/usr/bin/writ-tauri --login\nStartupNotify=false\nTerminal=false";

fn facts(legacy_entry: Option<&str>) -> AutostartFacts<'_> {
    AutostartFacts {
        legacy_entry,
        current_present: false,
        app_name: "Jotra",
        app_path: Some("/usr/bin/jotra-tauri"),
    }
}

fn replaced(plan: AutostartPlan) -> String {
    match plan {
        AutostartPlan::Replace { entry } => entry,
        other => panic!("expected a replacement, got {other:?}"),
    }
}

#[test]
fn the_autostart_entry_names_follow_the_product() {
    assert_eq!(autostart_file_name("Writ"), "Writ.desktop");
    assert_eq!(autostart_file_name("Jotra"), "Jotra.desktop");
    assert_eq!(AUTOSTART_FOLDER, ".config/autostart");
}

#[test]
fn the_plugins_entry_moves_with_the_new_exec_name_and_comment() {
    let entry = replaced(plan_autostart(&facts(Some(PLUGIN_ENTRY))));

    assert_eq!(
        entry,
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Jotra\nComment=Jotra startup script\nExec=/usr/bin/jotra-tauri --login\nStartupNotify=false\nTerminal=false"
    );
}

#[test]
fn a_turned_off_entry_stays_turned_off() {
    let legacy = format!("{PLUGIN_ENTRY}\nHidden=true\nX-GNOME-Autostart-enabled=false\n");

    let entry = replaced(plan_autostart(&facts(Some(&legacy))));

    assert_eq!(desktop_entry_value(&entry, "Hidden"), Some("true"));
    assert_eq!(
        desktop_entry_value(&entry, "X-GNOME-Autostart-enabled"),
        Some("false")
    );
    assert_eq!(
        desktop_entry_value(&entry, "Exec"),
        Some("/usr/bin/jotra-tauri --login")
    );
}

#[test]
fn a_name_the_person_changed_is_kept() {
    let legacy = PLUGIN_ENTRY
        .replace("Name=Writ", "Name=Notes")
        .replace("Comment=Writ startup script", "Comment=mine");

    let entry = replaced(plan_autostart(&facts(Some(&legacy))));

    assert_eq!(desktop_entry_value(&entry, "Name"), Some("Notes"));
    assert_eq!(desktop_entry_value(&entry, "Comment"), Some("mine"));
}

#[test]
fn an_appimage_path_with_a_space_is_quoted() {
    let entry = replaced(plan_autostart(&AutostartFacts {
        app_path: Some("/home/a/My Apps/Writ.AppImage"),
        ..facts(Some(PLUGIN_ENTRY))
    }));

    assert_eq!(
        desktop_entry_value(&entry, "Exec"),
        Some("\"/home/a/My Apps/Writ.AppImage\" --login")
    );
}

#[test]
fn only_the_word_that_starts_the_app_changes() {
    let legacy = PLUGIN_ENTRY.replace(
        "Exec=/usr/bin/writ-tauri --login",
        "Exec=env GDK_SCALE=2 /usr/bin/writ-tauri --login %U",
    );

    let entry = replaced(plan_autostart(&facts(Some(&legacy))));

    assert_eq!(
        desktop_entry_value(&entry, "Exec"),
        Some("env GDK_SCALE=2 /usr/bin/jotra-tauri --login %U")
    );
}

#[test]
fn an_entry_whose_exec_starts_another_program_is_left_alone() {
    for exec in [
        "/opt/other/launcher --login",
        "/opt/other/writ-tauri-helper --login",
        "bin/writ-tauri --login",
        "\"/unterminated --login",
    ] {
        let legacy = PLUGIN_ENTRY.replace("/usr/bin/writ-tauri --login", exec);
        for current_present in [false, true] {
            assert_eq!(
                plan_autostart(&AutostartFacts {
                    current_present,
                    ..facts(Some(&legacy))
                }),
                AutostartPlan::ForeignProgram,
                "{exec}"
            );
        }
    }
}

#[test]
fn an_unquoted_image_path_with_a_space_is_written_quoted() {
    let image = "/home/a/My Apps/Writ.AppImage";
    let legacy = PLUGIN_ENTRY.replace("/usr/bin/writ-tauri", image);

    let entry = replaced(plan_autostart(&AutostartFacts {
        app_path: Some(image),
        ..facts(Some(&legacy))
    }));

    assert_eq!(
        desktop_entry_value(&entry, "Exec"),
        Some("\"/home/a/My Apps/Writ.AppImage\" --login")
    );
}

#[test]
fn a_path_that_needs_escaping_moves_nothing() {
    for app_path in [
        Some("/home/a/$HOME/Writ.AppImage"),
        Some("/opt/100%/x"),
        None,
    ] {
        assert_eq!(
            plan_autostart(&AutostartFacts {
                app_path,
                ..facts(Some(PLUGIN_ENTRY))
            }),
            AutostartPlan::PathNotWritable
        );
    }
}

#[test]
fn nothing_moves_without_an_entry_of_jotras() {
    assert_eq!(plan_autostart(&facts(None)), AutostartPlan::Untouched);
    let other = PLUGIN_ENTRY.replace(" --login", "");
    assert_eq!(
        plan_autostart(&facts(Some(&other))),
        AutostartPlan::Untouched
    );
    let no_group = PLUGIN_ENTRY.replace("[Desktop Entry]", "[Other]");
    assert_eq!(
        plan_autostart(&facts(Some(&no_group))),
        AutostartPlan::Untouched
    );
}

#[test]
fn an_existing_current_entry_takes_over_and_the_old_one_goes() {
    assert_eq!(
        plan_autostart(&AutostartFacts {
            current_present: true,
            ..facts(Some(PLUGIN_ENTRY))
        }),
        AutostartPlan::RemoveLegacy
    );
}

#[test]
fn a_build_under_the_old_name_moves_nothing() {
    assert_eq!(
        plan_autostart(&AutostartFacts {
            app_name: "Writ",
            ..facts(Some(PLUGIN_ENTRY))
        }),
        AutostartPlan::Untouched
    );
}

#[test]
fn desktop_entry_values_are_read_from_the_main_group_only() {
    let entry =
        "[Desktop Entry]\nName[de]=Schreib\nName = Writ\n[Desktop Action new]\nExec=other\n";

    assert_eq!(desktop_entry_value(entry, "Name"), Some("Writ"));
    assert_eq!(desktop_entry_value(entry, "Exec"), None);
}

#[test]
fn setting_a_value_keeps_every_other_line() {
    let entry = "# made by hand\r\n[Desktop Entry]\r\nName=Writ\r\nName[de]=Schreib\r\n[Desktop Action new]\r\nName=New\r\n";

    assert_eq!(
        set_desktop_entry_value(entry, "Name", "Jotra").as_deref(),
        Some("# made by hand\r\n[Desktop Entry]\r\nName=Jotra\r\nName[de]=Schreib\r\n[Desktop Action new]\r\nName=New\r\n")
    );
    assert_eq!(
        set_desktop_entry_value(entry, "Exec", "x").as_deref(),
        Some("# made by hand\r\n[Desktop Entry]\r\nName=Writ\r\nName[de]=Schreib\r\nExec=x\n[Desktop Action new]\r\nName=New\r\n")
    );
    assert_eq!(
        set_desktop_entry_value("[Desktop Entry]\nType=Application", "Exec", "x").as_deref(),
        Some("[Desktop Entry]\nType=Application\nExec=x")
    );
    assert_eq!(
        set_desktop_entry_value("[Desktop Entry]\n", "Exec", "x").as_deref(),
        Some("[Desktop Entry]\nExec=x\n")
    );
    assert_eq!(set_desktop_entry_value("Name=Writ\n", "Name", "x"), None);
}

#[test]
fn a_program_is_written_bare_quoted_or_refused() {
    assert_eq!(
        exec_argument("/usr/bin/jotra-tauri").as_deref(),
        Some("/usr/bin/jotra-tauri")
    );
    assert_eq!(exec_argument("/a b/c").as_deref(), Some("\"/a b/c\""));
    assert_eq!(exec_argument("/a'b").as_deref(), Some("\"/a'b\""));
    for refused in [
        "", "/a\"b", "/a`b", "/a$b", "/a\\b", "/a%b", "/a\nb", "/a\tb",
    ] {
        assert_eq!(exec_argument(refused), None, "{refused:?}");
    }
}

#[test]
fn the_app_word_is_replaced_and_every_other_word_kept() {
    assert_eq!(
        exec_with_app_path(
            r#"env A=1 "/opt/My Apps/writ-tauri" --login %U"#,
            "/usr/bin/jotra-tauri"
        )
        .as_deref(),
        Some("env A=1 /usr/bin/jotra-tauri --login %U")
    );
    assert_eq!(
        exec_with_app_path(r#""/a/My \"Apps\"/x" --login"#, r#"/a/My "Apps"/x"#),
        None,
        "a path that needs escaping is refused"
    );
    assert_eq!(
        exec_with_app_path("/a/My Apps/x --login", "/a/My Apps/x").as_deref(),
        Some("\"/a/My Apps/x\" --login")
    );
    assert_eq!(
        exec_with_app_path("jotra-tauri --login", "/usr/bin/jotra-tauri").as_deref(),
        Some("/usr/bin/jotra-tauri --login")
    );
    assert_eq!(
        exec_with_app_path(r#""/unterminated/writ-tauri"#, "/usr/bin/jotra-tauri"),
        None
    );
    assert_eq!(
        exec_with_app_path("/usr/bin/other --login", "/new/app"),
        None
    );
    assert_eq!(exec_with_app_path("", "/new/app"), None);
}

#[test]
fn the_applications_folder_follows_xdg_data_home() {
    let home = Path::new("/home/a");
    assert_eq!(
        user_applications_dir(None, Some(home)),
        Some(PathBuf::from("/home/a/.local/share/applications"))
    );
    assert_eq!(
        user_applications_dir(Some(" "), Some(home)),
        Some(PathBuf::from("/home/a/.local/share/applications"))
    );
    assert_eq!(
        user_applications_dir(Some("/data"), None),
        Some(PathBuf::from("/data/applications"))
    );
    assert_eq!(user_applications_dir(None, None), None);
}

const IMAGE: &str = "/home/a/.local/share/writ/Writ.AppImage";

#[test]
fn the_appimage_entry_keeps_its_id_with_a_new_name() {
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Writ\nExec={IMAGE} %F\nMimeType=text/plain;\n"
    );

    assert_eq!(APPIMAGE_ENTRY_FILE, "writ.desktop");
    assert_eq!(
        plan_appimage_entry(&entry, IMAGE, "Jotra"),
        Some(format!(
            "[Desktop Entry]\nType=Application\nName=Jotra\nExec={IMAGE} %F\nMimeType=text/plain;\n"
        ))
    );
}

#[test]
fn a_quoted_image_exec_is_rewritten_in_the_plain_form() {
    let entry = format!("[Desktop Entry]\nName=Writ\nExec=\"{IMAGE}\" %F\n");

    assert_eq!(
        plan_appimage_entry(&entry, IMAGE, "Jotra"),
        Some(format!("[Desktop Entry]\nName=Jotra\nExec={IMAGE} %F\n"))
    );
}

#[test]
fn an_unquoted_image_path_with_a_space_is_matched_and_quoted() {
    let image = "/home/a/My Apps/Writ.AppImage";
    let entry = format!("[Desktop Entry]\nName=Writ\nExec={image} %F\n");

    assert_eq!(
        plan_appimage_entry(&entry, image, "Jotra"),
        Some(format!(
            "[Desktop Entry]\nName=Jotra\nExec=\"{image}\" %F\n"
        ))
    );
    let longer = "[Desktop Entry]\nName=Writ\nExec=/home/a/My Apps/Writ.AppImage2 %F\n";
    assert_eq!(plan_appimage_entry(longer, image, "Jotra"), None);
}

#[test]
fn an_entry_for_another_program_or_already_current_is_left_alone() {
    let other = "[Desktop Entry]\nName=Writ\nExec=/usr/bin/other %F\n";
    assert_eq!(plan_appimage_entry(other, IMAGE, "Jotra"), None);
    let current = format!("[Desktop Entry]\nName=Jotra\nExec={IMAGE} %F\n");
    assert_eq!(plan_appimage_entry(&current, IMAGE, "Jotra"), None);
    assert_eq!(
        plan_appimage_entry("[Desktop Entry]\nName=Writ\n", IMAGE, "Jotra"),
        None
    );
}

#[test]
fn the_command_copies_are_under_both_names_in_both_places() {
    let home = Path::new("/home/a");

    assert_eq!(
        appimage_command_copies(Some(home), Path::new(IMAGE)),
        vec![
            CommandCopy {
                path: PathBuf::from("/home/a/.local/bin/writ"),
                legacy_name: true
            },
            CommandCopy {
                path: PathBuf::from("/home/a/.local/bin/jotra"),
                legacy_name: false
            },
            CommandCopy {
                path: PathBuf::from("/home/a/.local/share/writ/writ-cli"),
                legacy_name: true
            },
            CommandCopy {
                path: PathBuf::from("/home/a/.local/share/writ").join(CLI_BINARY_STEM),
                legacy_name: false
            },
        ]
    );
    assert_eq!(
        appimage_command_copies(None, Path::new(IMAGE)).len(),
        2,
        "without a home folder only the copies beside the image remain"
    );
}

#[test]
fn only_a_signed_copy_that_differs_is_stale() {
    let bundled = [b"new ".as_slice(), CLI_SIGNATURE].concat();
    let older = [b"old ".as_slice(), CLI_SIGNATURE].concat();

    assert!(is_stale_command_copy(&older, &bundled));
    assert!(!is_stale_command_copy(&bundled, &bundled));
    assert!(!is_stale_command_copy(
        b"#!/bin/sh\nexec writ-cli \"$@\"\n",
        &bundled
    ));
}
