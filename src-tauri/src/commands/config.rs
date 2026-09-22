use crate::poison::recover_poison;
use crate::state::AppState;
use std::time::Instant;
use tauri::State;
use writ_core::config::WritConfig;

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Result<WritConfig, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    Ok(config.clone())
}

/// Serializes and writes `config` to disk, recording the write in the
/// watcher ignore set so the change is not re-surfaced as external.
///
/// The key is the config file's canonical path under the config namespace,
/// which is what the config watcher looks up. The bare `config.toml` it used
/// to be was shared with every note of that name (ADR-028 section 6).
pub(crate) fn persist_config(state: &AppState, config: &WritConfig) -> Result<(), String> {
    let contents = state
        .config_store
        .serialize(config)
        .map_err(|e| e.to_string())?;

    let key = writ_core::watcher::ignore::config_key(&crate::watcher::handler::ignore_key_path(
        state.config_store.path(),
    ));

    {
        let mut ignore = recover_poison(
            state.watcher_ignore.lock(),
            "commands::config::persist_config",
        );
        ignore.record(key, contents.as_bytes(), Instant::now());
    }

    state
        .config_store
        .write_serialized(&contents)
        .map_err(|e| e.to_string())
}

/// Takes a frontend write into the live config and answers what belongs on
/// disk.
///
/// The frontend sends its whole copy of the settings, which may have been read
/// before a command wrote a field only Rust writes, so the fields Rust owns are
/// carried over from the live value rather than taken from the write
/// (`WritConfig::carry_rust_owned`). Both steps happen against the same
/// borrowed value, so a consent recorded between a read and this write cannot
/// be lost in between.
fn merge_into_live(live: &mut WritConfig, incoming: WritConfig) -> WritConfig {
    let mut merged = incoming;
    merged.carry_rust_owned(live);
    *live = merged.clone();
    merged
}

/// IPC: takes the frontend's whole config, writes it, and draws the menu bar
/// again when the write turned an app on or off (ADR-042 section 3).
#[tauri::command]
pub fn update_config(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    config: WritConfig,
) -> Result<(), String> {
    let (merged, previous) = {
        let mut current = state.config.lock().map_err(|e| e.to_string())?;
        let previous = current.clone();
        (merge_into_live(&mut current, config), previous)
    };

    if let Err(reason) = persist_config(&state, &merged) {
        // Memory goes back to what the write found, except for the fields Rust
        // owns: a consent or an approval recorded while the write was on its
        // way to disk is read from the live value rather than rolled back with
        // it.
        let mut current = state.config.lock().map_err(|e| e.to_string())?;
        let mut restored = previous;
        restored.carry_rust_owned(&current);
        *current = restored;
        return Err(reason);
    }
    crate::app_menu::sync(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defect an operator hit: a consent was recorded, accepted and
    /// written, and the next settings write from the frontend — a window size,
    /// a pane width, anything — carried a copy read before it and emptied
    /// `consented_hosts` on disk. The merge is what makes that write harmless.
    #[test]
    fn a_stale_write_after_a_consent_keeps_the_consented_host() {
        let mut live = WritConfig::default();
        live.ai.provider = "deepseek".to_string();
        live.ai.consented_hosts = vec!["api.deepseek.com".to_string()];

        let mut stale = WritConfig::default();
        stale.ai.provider = "deepseek".to_string();
        stale.window.width = 880;

        let persisted = merge_into_live(&mut live, stale);

        assert_eq!(persisted.ai.consented_hosts, ["api.deepseek.com"]);
        assert_eq!(live.ai.consented_hosts, ["api.deepseek.com"]);
        // The write's own change is what the file takes.
        assert_eq!(persisted.window.width, 880);
        assert_eq!(live.window.width, 880);
    }
}
