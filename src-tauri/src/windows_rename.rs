//! The Windows login item and taskbar id after the rename (ADR-059 decision
//! 10), carried out over the plan in [`jotra_core::windows_rename`].

use std::path::PathBuf;

use jotra_core::legacy_names;
use jotra_core::name_migration::MigrationStep;
use jotra_core::windows_rename::{plan_run_value, RunValueFacts, RunValuePlan};
use jotra_storage::name_migration::{run_migration_step, StepCompletion, StepRun};
use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// Why moving the login item did not finish.
#[derive(Debug, thiserror::Error)]
#[error("{operation} {key}\\{name}: {reason}")]
pub(crate) struct WindowsRenameError {
    operation: &'static str,
    key: &'static str,
    name: String,
    reason: String,
}

impl WindowsRenameError {
    pub(crate) fn new(
        operation: &'static str,
        key: &'static str,
        name: &str,
        reason: impl std::fmt::Display,
    ) -> Self {
        Self {
            operation,
            key,
            name: name.to_owned(),
            reason: reason.to_string(),
        }
    }
}

/// The per-user registry values the login item lives in: its command under
/// the `Run` key and its on/off state under `StartupApproved\Run`. A value or
/// key that is absent reads as `None` and removes as done.
pub(crate) trait RunRegistry {
    /// The command registered under `name`.
    fn run_command(&self, name: &str) -> Result<Option<String>, WindowsRenameError>;
    /// Registers `command` under `name`.
    fn set_run_command(&self, name: &str, command: &str) -> Result<(), WindowsRenameError>;
    /// Removes the command registered under `name`.
    fn remove_run_command(&self, name: &str) -> Result<(), WindowsRenameError>;
    /// The on/off state recorded for `name`.
    fn approval(&self, name: &str) -> Result<Option<Vec<u8>>, WindowsRenameError>;
    /// Records `state` as the on/off state for `name`.
    fn set_approval(&self, name: &str, state: &[u8]) -> Result<(), WindowsRenameError>;
    /// Removes the on/off state recorded for `name`.
    fn remove_approval(&self, name: &str) -> Result<(), WindowsRenameError>;
}

/// Moves the login item from the old product name to `current_name`: the
/// on/off state first, then the command, then the old command and the old
/// state go.
pub(crate) fn migrate_run_value(
    registry: &impl RunRegistry,
    current_name: &str,
) -> Result<StepCompletion, WindowsRenameError> {
    let legacy_command = registry.run_command(legacy_names::PRODUCT_NAME)?;
    let current_present = registry.run_command(current_name)?.is_some();
    let legacy_approval = registry.approval(legacy_names::PRODUCT_NAME)?;
    let plan = plan_run_value(&RunValueFacts {
        legacy_command: legacy_command.as_deref(),
        current_present,
        legacy_approval: legacy_approval.as_deref(),
        current_name,
    });
    match plan {
        RunValuePlan::Untouched => {}
        RunValuePlan::RemoveLegacy => registry.remove_run_command(legacy_names::PRODUCT_NAME)?,
        RunValuePlan::Move { command, approval } => {
            if let Some(approval) = &approval {
                registry.set_approval(current_name, approval)?;
            }
            registry.set_run_command(current_name, &command)?;
            registry.remove_run_command(legacy_names::PRODUCT_NAME)?;
            if approval.is_some() {
                registry.remove_approval(legacy_names::PRODUCT_NAME)?;
            }
        }
    }
    Ok(StepCompletion::Done)
}

/// Runs [`migrate_run_value`] once per the record in `data_dir`.
pub(crate) fn run_rename_step(
    registry: &impl RunRegistry,
    data_dir: &std::path::Path,
    version: &str,
    current_name: &str,
) -> StepRun {
    run_migration_step(data_dir, MigrationStep::WindowsRunValue, version, || {
        migrate_run_value(registry, current_name)
    })
}

/// Runs the login item step off the main thread in a release build.
pub(crate) fn spawn_rename_step(handle: &AppHandle, registry: impl RunRegistry + Send + 'static) {
    if cfg!(debug_assertions) {
        return;
    }
    let package = handle.package_info();
    let data_dir: PathBuf = handle.state::<AppState>().jotra_dir.clone();
    let version = package.version.to_string();
    let name = package.name.clone();
    std::thread::spawn(move || {
        run_rename_step(&registry, &data_dir, &version, &name);
    });
}

#[cfg(target_os = "windows")]
pub(crate) use current_user::{set_app_user_model_id, CurrentUserRegistry};

#[cfg(target_os = "windows")]
mod current_user {
    use jotra_core::windows_rename::{wide_nul, APP_USER_MODEL_ID, RUN_KEY, STARTUP_APPROVED_KEY};
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    use windows_registry::{Key, Type, CURRENT_USER};

    use super::{RunRegistry, WindowsRenameError};

    const NOT_FOUND: i32 = 0x8007_0002_u32 as i32;

    fn found<T>(result: windows_registry::Result<T>) -> windows_registry::Result<Option<T>> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.code().0 == NOT_FOUND => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn read_string(path: &str, name: &str) -> windows_registry::Result<Option<String>> {
        match found(CURRENT_USER.open(path))? {
            Some(key) => found(key.get_string(name)),
            None => Ok(None),
        }
    }

    fn read_bytes(path: &str, name: &str) -> windows_registry::Result<Option<Vec<u8>>> {
        let Some(key) = found(CURRENT_USER.open(path))? else {
            return Ok(None);
        };
        let Some(value) = found(key.get_value(name))? else {
            return Ok(None);
        };
        Ok(Some(value.to_vec()))
    }

    fn remove_value(path: &str, name: &str) -> windows_registry::Result<()> {
        let Some(key) = found(CURRENT_USER.options().read().write().open(path))? else {
            return Ok(());
        };
        found(key.remove_value(name))?;
        Ok(())
    }

    fn create_key(path: &str) -> windows_registry::Result<Key> {
        CURRENT_USER.create(path)
    }

    /// The signed-in user's registry hive.
    pub(crate) struct CurrentUserRegistry;

    impl RunRegistry for CurrentUserRegistry {
        fn run_command(&self, name: &str) -> Result<Option<String>, WindowsRenameError> {
            read_string(RUN_KEY, name)
                .map_err(|error| WindowsRenameError::new("read", RUN_KEY, name, error))
        }

        fn set_run_command(&self, name: &str, command: &str) -> Result<(), WindowsRenameError> {
            create_key(RUN_KEY)
                .and_then(|key| key.set_string(name, command))
                .map_err(|error| WindowsRenameError::new("write", RUN_KEY, name, error))
        }

        fn remove_run_command(&self, name: &str) -> Result<(), WindowsRenameError> {
            remove_value(RUN_KEY, name)
                .map_err(|error| WindowsRenameError::new("remove", RUN_KEY, name, error))
        }

        fn approval(&self, name: &str) -> Result<Option<Vec<u8>>, WindowsRenameError> {
            read_bytes(STARTUP_APPROVED_KEY, name)
                .map_err(|error| WindowsRenameError::new("read", STARTUP_APPROVED_KEY, name, error))
        }

        fn set_approval(&self, name: &str, state: &[u8]) -> Result<(), WindowsRenameError> {
            create_key(STARTUP_APPROVED_KEY)
                .and_then(|key| key.set_bytes(name, Type::Bytes, state))
                .map_err(|error| {
                    WindowsRenameError::new("write", STARTUP_APPROVED_KEY, name, error)
                })
        }

        fn remove_approval(&self, name: &str) -> Result<(), WindowsRenameError> {
            remove_value(STARTUP_APPROVED_KEY, name).map_err(|error| {
                WindowsRenameError::new("remove", STARTUP_APPROVED_KEY, name, error)
            })
        }
    }

    /// Gives the process the app id the Start Menu shortcut carries, so its
    /// windows group with taskbar pins made from either shortcut. Called
    /// before any window exists.
    pub(crate) fn set_app_user_model_id() -> windows::core::Result<()> {
        let id = wide_nul(APP_USER_MODEL_ID);
        // SAFETY: `id` is a nul-terminated UTF-16 string that outlives the
        // call, which copies it.
        unsafe { SetCurrentProcessExplicitAppUserModelID(PCWSTR(id.as_ptr())) }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use jotra_core::name_migration::StepOutcome;
    use jotra_storage::name_migration::read_name_migration_record;

    use super::*;

    const LEGACY_COMMAND: &str = r"C:\Program Files\Writ\writ-tauri.exe --login";
    const ENABLED: [u8; 12] = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    const DISABLED: [u8; 12] = [3, 0, 0, 0, 0x10, 0x20, 0, 0, 0, 0, 0, 0];

    #[derive(Default)]
    struct FakeRegistry {
        run: RefCell<BTreeMap<String, String>>,
        approved: RefCell<BTreeMap<String, Vec<u8>>>,
        calls: RefCell<Vec<String>>,
        fail_on: Option<&'static str>,
    }

    impl FakeRegistry {
        fn with_legacy(approval: Option<&[u8]>) -> Self {
            let registry = Self::default();
            registry
                .run
                .borrow_mut()
                .insert("Writ".to_owned(), LEGACY_COMMAND.to_owned());
            if let Some(approval) = approval {
                registry
                    .approved
                    .borrow_mut()
                    .insert("Writ".to_owned(), approval.to_vec());
            }
            registry
        }

        fn call(&self, call: String) -> Result<(), WindowsRenameError> {
            let failing = self.fail_on.is_some_and(|prefix| call.starts_with(prefix));
            self.calls.borrow_mut().push(call.clone());
            if failing {
                return Err(WindowsRenameError::new(
                    "write",
                    "Run",
                    &call,
                    "access denied",
                ));
            }
            Ok(())
        }
    }

    impl RunRegistry for FakeRegistry {
        fn run_command(&self, name: &str) -> Result<Option<String>, WindowsRenameError> {
            Ok(self.run.borrow().get(name).cloned())
        }

        fn set_run_command(&self, name: &str, command: &str) -> Result<(), WindowsRenameError> {
            self.call(format!("set run {name}"))?;
            self.run
                .borrow_mut()
                .insert(name.to_owned(), command.to_owned());
            Ok(())
        }

        fn remove_run_command(&self, name: &str) -> Result<(), WindowsRenameError> {
            self.call(format!("remove run {name}"))?;
            self.run.borrow_mut().remove(name);
            Ok(())
        }

        fn approval(&self, name: &str) -> Result<Option<Vec<u8>>, WindowsRenameError> {
            Ok(self.approved.borrow().get(name).cloned())
        }

        fn set_approval(&self, name: &str, state: &[u8]) -> Result<(), WindowsRenameError> {
            self.call(format!("set approval {name}"))?;
            self.approved
                .borrow_mut()
                .insert(name.to_owned(), state.to_vec());
            Ok(())
        }

        fn remove_approval(&self, name: &str) -> Result<(), WindowsRenameError> {
            self.call(format!("remove approval {name}"))?;
            self.approved.borrow_mut().remove(name);
            Ok(())
        }
    }

    #[test]
    fn the_login_item_moves_with_its_state_before_the_old_value_goes() {
        let registry = FakeRegistry::with_legacy(Some(&DISABLED));

        assert_eq!(
            migrate_run_value(&registry, "Jotra").expect("migrate"),
            StepCompletion::Done
        );

        assert_eq!(
            registry.run.borrow().get("Jotra").map(String::as_str),
            Some(LEGACY_COMMAND)
        );
        assert!(!registry.run.borrow().contains_key("Writ"));
        assert_eq!(
            registry.approved.borrow().get("Jotra").map(Vec::as_slice),
            Some(DISABLED.as_slice())
        );
        assert!(!registry.approved.borrow().contains_key("Writ"));
        assert_eq!(
            *registry.calls.borrow(),
            [
                "set approval Jotra",
                "set run Jotra",
                "remove run Writ",
                "remove approval Writ"
            ]
        );
    }

    #[test]
    fn a_login_item_without_a_state_moves_alone() {
        let registry = FakeRegistry::with_legacy(None);

        migrate_run_value(&registry, "Jotra").expect("migrate");

        assert!(registry.approved.borrow().is_empty());
        assert_eq!(
            *registry.calls.borrow(),
            ["set run Jotra", "remove run Writ"]
        );
    }

    #[test]
    fn an_existing_current_value_stays_and_the_old_one_goes() {
        let registry = FakeRegistry::with_legacy(Some(&ENABLED));
        registry
            .run
            .borrow_mut()
            .insert("Jotra".to_owned(), "current --login".to_owned());

        migrate_run_value(&registry, "Jotra").expect("migrate");

        assert_eq!(
            registry.run.borrow().get("Jotra").map(String::as_str),
            Some("current --login")
        );
        assert!(!registry.run.borrow().contains_key("Writ"));
        assert_eq!(*registry.calls.borrow(), ["remove run Writ"]);
    }

    #[test]
    fn another_programs_value_is_left_alone() {
        for command in [r"C:\Tools\writ.exe", r"C:\Tools\other.exe --login"] {
            let registry = FakeRegistry::default();
            registry
                .run
                .borrow_mut()
                .insert("Writ".to_owned(), command.to_owned());
            registry
                .approved
                .borrow_mut()
                .insert("Writ".to_owned(), DISABLED.to_vec());

            migrate_run_value(&registry, "Jotra").expect("migrate");

            assert!(registry.calls.borrow().is_empty(), "{command}");
            assert!(registry.run.borrow().contains_key("Writ"));
            assert!(registry.approved.borrow().contains_key("Writ"));
        }
    }

    #[test]
    fn a_failed_write_keeps_the_old_value() {
        let registry = FakeRegistry {
            fail_on: Some("set run"),
            ..FakeRegistry::with_legacy(Some(&ENABLED))
        };

        let error = migrate_run_value(&registry, "Jotra").expect_err("write fails");

        assert!(error.to_string().contains("access denied"));
        assert!(registry.run.borrow().contains_key("Writ"));
        assert!(!registry.run.borrow().contains_key("Jotra"));
        assert!(registry.approved.borrow().contains_key("Writ"));
    }

    #[test]
    fn the_step_is_recorded_and_runs_once() {
        let data = tempfile::tempdir().expect("data");
        let registry = FakeRegistry::with_legacy(Some(&ENABLED));

        assert_eq!(
            run_rename_step(&registry, data.path(), "1.1.0", "Jotra"),
            StepRun::Done
        );
        registry
            .run
            .borrow_mut()
            .insert("Writ".to_owned(), LEGACY_COMMAND.to_owned());
        assert_eq!(
            run_rename_step(&registry, data.path(), "1.1.0", "Jotra"),
            StepRun::AlreadySettled
        );

        assert!(registry.run.borrow().contains_key("Writ"));
        assert_eq!(
            read_name_migration_record(data.path())
                .entry(MigrationStep::WindowsRunValue)
                .map(|entry| entry.outcome),
            Some(StepOutcome::Done)
        );
    }

    #[test]
    fn a_failed_step_is_recorded_with_its_reason() {
        let data = tempfile::tempdir().expect("data");
        let registry = FakeRegistry {
            fail_on: Some("remove run"),
            ..FakeRegistry::with_legacy(None)
        };

        assert_eq!(
            run_rename_step(&registry, data.path(), "1.1.0", "Jotra"),
            StepRun::Failed
        );

        let entry = read_name_migration_record(data.path())
            .entry(MigrationStep::WindowsRunValue)
            .expect("entry");
        assert_eq!(entry.outcome, StepOutcome::Failed);
        assert!(entry
            .reason
            .is_some_and(|reason| reason.contains("access denied")));
    }
}
