use jotra_core::startup::{is_login_command_line, GUI_BINARY_STEM};
use jotra_core::windows_rename::{
    plan_run_value, wide_nul, RunValueFacts, RunValuePlan, APP_USER_MODEL_ID, RUN_KEY,
    STARTUP_APPROVED_KEY,
};

const LEGACY_COMMAND: &str = r"C:\Program Files\Writ\writ-tauri.exe --login";
const ENABLED: [u8; 12] = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

fn facts(legacy_command: Option<&str>) -> RunValueFacts<'_> {
    RunValueFacts {
        legacy_command,
        current_present: false,
        legacy_approval: None,
        current_name: "Jotra",
    }
}

#[test]
fn the_login_item_moves_with_its_command_and_approval() {
    let plan = plan_run_value(&RunValueFacts {
        legacy_approval: Some(&ENABLED),
        ..facts(Some(LEGACY_COMMAND))
    });

    assert_eq!(
        plan,
        RunValuePlan::Move {
            command: LEGACY_COMMAND.to_owned(),
            approval: Some(ENABLED.to_vec()),
        }
    );
}

#[test]
fn a_login_item_without_a_recorded_state_moves_without_one() {
    assert_eq!(
        plan_run_value(&facts(Some(LEGACY_COMMAND))),
        RunValuePlan::Move {
            command: LEGACY_COMMAND.to_owned(),
            approval: None,
        }
    );
}

#[test]
fn nothing_moves_without_a_value_under_the_old_name() {
    assert_eq!(plan_run_value(&facts(None)), RunValuePlan::Untouched);
}

#[test]
fn another_programs_value_under_the_old_name_is_left_alone() {
    assert_eq!(
        plan_run_value(&facts(Some(r"C:\Tools\writ.exe --tray"))),
        RunValuePlan::Untouched
    );
    assert_eq!(
        plan_run_value(&facts(Some(r"C:\Tools\writ.exe --login-shell"))),
        RunValuePlan::Untouched
    );
}

#[test]
fn a_login_value_for_another_program_is_left_alone() {
    for command in [
        r"C:\Tools\other.exe --login",
        r#""C:\Tools\other.exe" --login"#,
        r"C:\Tools\writ-tauri-helper.exe --login",
    ] {
        assert_eq!(
            plan_run_value(&facts(Some(command))),
            RunValuePlan::Untouched,
            "{command}"
        );
        assert_eq!(
            plan_run_value(&RunValueFacts {
                current_present: true,
                ..facts(Some(command))
            }),
            RunValuePlan::Untouched,
            "{command}"
        );
    }
}

#[test]
fn the_app_binary_under_either_name_moves() {
    assert_eq!(GUI_BINARY_STEM, "jotra-tauri");
    for command in [
        r#""C:\Program Files\Jotra\jotra-tauri.exe" --login"#,
        r"C:\PROGRA~1\Writ\WRIT-TAURI.EXE --login",
        r"C:\Apps\writ-tauri --login",
        r"C:\My Apps\Writ\writ-tauri.exe --login",
    ] {
        assert_eq!(
            plan_run_value(&facts(Some(command))),
            RunValuePlan::Move {
                command: command.to_owned(),
                approval: None,
            },
            "{command}"
        );
    }
}

#[test]
fn an_existing_current_value_takes_over_and_the_old_one_goes() {
    assert_eq!(
        plan_run_value(&RunValueFacts {
            current_present: true,
            legacy_approval: Some(&ENABLED),
            ..facts(Some(LEGACY_COMMAND))
        }),
        RunValuePlan::RemoveLegacy
    );
}

#[test]
fn a_build_under_the_old_name_moves_nothing() {
    assert_eq!(
        plan_run_value(&RunValueFacts {
            current_name: "Writ",
            ..facts(Some(LEGACY_COMMAND))
        }),
        RunValuePlan::Untouched
    );
}

#[test]
fn the_login_argument_is_found_quoted_or_bare() {
    assert!(is_login_command_line(LEGACY_COMMAND));
    assert!(is_login_command_line(
        r#""C:\Program Files\Writ\writ-tauri.exe" "--login""#
    ));
    assert!(is_login_command_line("/usr/bin/writ-tauri --login"));
    assert!(!is_login_command_line("/usr/bin/writ-tauri"));
    assert!(!is_login_command_line("/usr/bin/writ-tauri --login=no"));
}

#[test]
fn the_registry_keys_are_the_per_user_sign_in_keys() {
    assert_eq!(RUN_KEY, r"Software\Microsoft\Windows\CurrentVersion\Run");
    assert_eq!(
        STARTUP_APPROVED_KEY,
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
    );
}

#[test]
fn the_app_id_stays_the_kept_identifier() {
    assert_eq!(APP_USER_MODEL_ID, "com.writ.editor");
}

#[test]
fn wide_strings_end_in_one_nul() {
    assert_eq!(wide_nul("ab"), vec![u16::from(b'a'), u16::from(b'b'), 0]);
    assert_eq!(wide_nul(""), vec![0]);
    let id = wide_nul(APP_USER_MODEL_ID);
    assert_eq!(id.len(), APP_USER_MODEL_ID.len() + 1);
    assert_eq!(id.iter().filter(|unit| **unit == 0).count(), 1);
}
