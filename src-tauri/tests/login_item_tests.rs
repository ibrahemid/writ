//! Open at login: the OS is asked first, and config.toml records only what it
//! gave (U44).

use std::cell::{Cell, RefCell};
use std::sync::Mutex as StdMutex;

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_tauri_lib::commands::login_item::{
    apply_first_run_choice, set_launch_at_login_inner, LoginItem, LoginItemError, LoginItemStatus,
};
use writ_tauri_lib::state::AppState;

static ENV_LOCK: StdMutex<()> = StdMutex::new(());

fn launch(data_dir: &std::path::Path, notes_dir: &std::path::Path) -> AppState {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("WRIT_DATA_DIR", data_dir);
    std::env::set_var("WRIT_NOTES_DIR", notes_dir);
    let state = AppState::initialize().expect("app state");
    std::env::remove_var("WRIT_DATA_DIR");
    std::env::remove_var("WRIT_NOTES_DIR");
    state
}

/// A login item that records each change and refuses when told to.
#[derive(Default)]
struct FakeLoginItem {
    refuse: bool,
    enabled: Cell<bool>,
    asks: RefCell<Vec<bool>>,
}

impl LoginItem for FakeLoginItem {
    fn is_enabled(&self) -> Result<bool, LoginItemError> {
        Ok(self.enabled.get())
    }

    fn set(&self, on: bool) -> Result<(), LoginItemError> {
        self.asks.borrow_mut().push(on);
        if self.refuse {
            return Err(LoginItemError::Os("not permitted".to_string()));
        }
        self.enabled.set(on);
        Ok(())
    }
}

fn on_disk(data: &TempDir) -> WritConfig {
    let text = std::fs::read_to_string(data.path().join("config.toml")).expect("config written");
    WritConfig::parse(&text).expect("config parses")
}

fn with_config(data: &TempDir, text: &str) {
    std::fs::write(data.path().join("config.toml"), text).expect("seed config");
}

#[test]
fn turning_it_off_asks_the_os_then_records_the_choice() {
    let (data, notes) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    with_config(&data, "[general]\nlaunch_at_login = true\n");
    let state = launch(data.path(), notes.path());
    let login = FakeLoginItem::default();
    login.enabled.set(true);

    let status = set_launch_at_login_inner(&state, &login, false).expect("turned off");

    assert_eq!(status, LoginItemStatus { enabled: false });
    assert_eq!(*login.asks.borrow(), [false]);
    assert!(!login.enabled.get());
    assert!(!state.config.lock().unwrap().general.launch_at_login);
    assert!(!on_disk(&data).general.launch_at_login);
}

#[test]
fn a_refusal_from_the_os_leaves_the_config_as_it_was() {
    let (data, notes) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    with_config(&data, "[general]\nlaunch_at_login = false\n");
    let state = launch(data.path(), notes.path());
    let login = FakeLoginItem {
        refuse: true,
        ..FakeLoginItem::default()
    };

    let error = set_launch_at_login_inner(&state, &login, true).expect_err("refused");

    assert!(matches!(error, LoginItemError::Os(_)));
    assert!(error.to_string().starts_with("ERR_LOGIN_ITEM"), "{error}");
    assert!(!state.config.lock().unwrap().general.launch_at_login);
    assert!(!on_disk(&data).general.launch_at_login);
}

#[test]
fn a_first_launch_turns_it_on() {
    let (data, notes) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let state = launch(data.path(), notes.path());
    assert!(state.first_run, "no config file is a first launch");
    let login = FakeLoginItem::default();

    assert!(apply_first_run_choice(&state, &login, false).expect("applied"));
    assert!(login.enabled.get());
}

#[test]
fn a_debug_build_never_makes_the_entry() {
    let (data, notes) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let state = launch(data.path(), notes.path());
    let login = FakeLoginItem::default();

    assert!(!apply_first_run_choice(&state, &login, true).expect("skipped"));
    assert!(login.asks.borrow().is_empty());
}

#[test]
fn a_config_that_says_off_makes_no_entry() {
    let (data, notes) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    with_config(&data, "[general]\nlaunch_at_login = false\n");
    let state = launch(data.path(), notes.path());
    let login = FakeLoginItem::default();

    assert!(!apply_first_run_choice(&state, &login, false).expect("skipped"));
    assert!(login.asks.borrow().is_empty());
}
