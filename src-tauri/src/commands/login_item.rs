//! Open at login: the OS entry that starts Writ hidden when the person logs
//! in, so the window chord works after a restart.
//!
//! The entry is made through tauri-plugin-autostart from Rust only; the
//! plugin's own commands are not granted to the webview. `general.launch_at_login`
//! records the choice, and only these commands write it, each after the OS
//! has answered.

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::{AutoLaunchManager, ManagerExt};
use tracing::{info, warn};

use crate::commands::config::mutate_config;
use crate::commands::offload::run_detached;
use crate::state::AppState;

/// The code an IPC failure of these commands carries.
pub const ERR_LOGIN_ITEM: &str = "ERR_LOGIN_ITEM";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoginItemError {
    #[error("{ERR_LOGIN_ITEM}: the OS did not change the login item ({0})")]
    Os(String),
    #[error("{ERR_LOGIN_ITEM}: the choice could not be written to config.toml ({0})")]
    Config(String),
}

/// What the OS is asked about the login item. A trait so the commands can be
/// tested without writing a real one.
pub trait LoginItem {
    fn is_enabled(&self) -> Result<bool, LoginItemError>;
    fn set(&self, on: bool) -> Result<(), LoginItemError>;
}

impl LoginItem for AutoLaunchManager {
    fn is_enabled(&self) -> Result<bool, LoginItemError> {
        AutoLaunchManager::is_enabled(self).map_err(|e| LoginItemError::Os(e.to_string()))
    }

    fn set(&self, on: bool) -> Result<(), LoginItemError> {
        let outcome = if on { self.enable() } else { self.disable() };
        outcome.map_err(|e| LoginItemError::Os(e.to_string()))
    }
}

/// Whether Writ opens at login, as the OS has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LoginItemStatus {
    pub enabled: bool,
}

/// Asks the OS for the login item, then records the choice. A refusal leaves
/// config.toml as it was.
pub fn set_launch_at_login_inner(
    state: &AppState,
    login: &impl LoginItem,
    on: bool,
) -> Result<LoginItemStatus, LoginItemError> {
    login.set(on)?;
    mutate_config(state, |config| {
        if config.general.launch_at_login == on {
            return false;
        }
        config.general.launch_at_login = on;
        true
    })
    .map_err(LoginItemError::Config)?;
    info!(on, "open at login changed");
    Ok(LoginItemStatus { enabled: on })
}

/// Makes the login item a first launch's config asks for (decision OD-11),
/// answering whether one was made.
///
/// A debug build makes none: the entry is named after the product, so a
/// development run would point the installed Writ's entry at itself.
pub fn apply_first_run_choice(
    state: &AppState,
    login: &impl LoginItem,
    debug_build: bool,
) -> Result<bool, LoginItemError> {
    let wanted = state
        .config
        .lock()
        .map_err(|e| LoginItemError::Config(e.to_string()))?
        .general
        .launch_at_login;
    if !wanted || debug_build {
        return Ok(false);
    }
    login.set(true)?;
    info!("open at login turned on by the first launch");
    Ok(true)
}

/// IPC: whether Writ opens at login. The OS's answer rather than the config's,
/// since the person can remove the entry from the system's own settings.
#[tauri::command]
pub async fn launch_at_login_status(app: AppHandle) -> Result<LoginItemStatus, String> {
    run_detached(ERR_LOGIN_ITEM, move || {
        let enabled = app
            .autolaunch()
            .inner()
            .is_enabled()
            .map_err(|e| e.to_string())?;
        Ok(LoginItemStatus { enabled })
    })
    .await
}

/// IPC: turns open at login on or off.
#[tauri::command]
pub async fn set_launch_at_login(app: AppHandle, on: bool) -> Result<LoginItemStatus, String> {
    run_detached(ERR_LOGIN_ITEM, move || {
        let login = app.autolaunch();
        set_launch_at_login_inner(&app.state::<AppState>(), login.inner(), on)
            .map_err(|e| e.to_string())
    })
    .await
}

/// Runs [`apply_first_run_choice`] off the main thread, after the first
/// launch's answers are written. A failure costs the login item only.
pub async fn apply_first_run_choice_detached(app: AppHandle) {
    let outcome = run_detached(ERR_LOGIN_ITEM, move || {
        let login = app.autolaunch();
        apply_first_run_choice(
            &app.state::<AppState>(),
            login.inner(),
            cfg!(debug_assertions),
        )
        .map_err(|e| e.to_string())
    })
    .await;
    if let Err(error) = outcome {
        warn!(%error, "the first launch could not turn on open at login");
    }
}
