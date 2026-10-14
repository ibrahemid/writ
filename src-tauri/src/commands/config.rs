use crate::poison::recover_poison;
use crate::state::AppState;
use std::time::Instant;
use tauri::State;
use writ_core::config::{ConfigDiagnostic, NotesConfig, WritConfig};

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Result<WritConfig, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    Ok(config.clone())
}

/// IPC: the values in config.toml the launch could not use, each with its key
/// and line, for the notice that names them. Empty once the file reads in
/// full.
#[tauri::command]
pub fn get_config_diagnostics(state: State<'_, AppState>) -> Vec<ConfigDiagnostic> {
    config_diagnostics(&state)
}

/// [`get_config_diagnostics`] without the IPC wrapper.
pub fn config_diagnostics(state: &AppState) -> Vec<ConfigDiagnostic> {
    recover_poison(
        state.config_diagnostics.lock(),
        "commands::config::config_diagnostics",
    )
    .clone()
}

/// What Settings hears when a folder move cannot be written into config.toml
/// because the file is not TOML, or holds a plain value where the folder
/// table belongs.
const FOLDER_CANNOT_BE_SAVED: &str =
    "Writ can't save a new folder to config.toml until the file is fixed.";

/// Whether the launch holds values in config.toml it could not read
/// ([`AppState::config_diagnostics`], ADR-048).
fn writes_are_held(state: &AppState) -> bool {
    !recover_poison(
        state.config_diagnostics.lock(),
        "commands::config::writes_are_held",
    )
    .is_empty()
}

/// Serializes and writes `config` to disk, recording the write in the
/// watcher ignore set so the change is not re-surfaced as external.
///
/// Writes nothing while the launch holds values it could not read
/// ([`AppState::config_diagnostics`], ADR-048): the running copy has defaults
/// in their place, and the file is the person's to fix. The change still
/// applies to the running copy, so it lasts the session.
pub(crate) fn persist_config(state: &AppState, config: &WritConfig) -> Result<(), String> {
    if writes_are_held(state) {
        tracing::info!("config.toml has values Writ could not read; change kept for this session");
        return Ok(());
    }
    let contents = state
        .config_store
        .serialize(config)
        .map_err(|e| e.to_string())?;
    write_config_text(state, &contents)
}

/// Writes a moved notes folder to disk, also while values are held.
///
/// The folder's files have already moved, so a path kept for the session
/// would be lost on the next launch, and a reload after the person fixes the
/// file would bring the old path back. While held, `notes.root` alone is put
/// into the file's text ([`NotesConfig::write_root_into`]), which leaves the
/// values the person has to fix as they wrote them.
pub(crate) fn persist_notes_root(state: &AppState, config: &WritConfig) -> Result<(), String> {
    if !writes_are_held(state) {
        return persist_config(state, config);
    }
    let root = config.notes.root.as_deref().unwrap_or_default();
    let contents = notes_root_in_file(state, root)?;
    write_config_text(state, &contents)
}

/// Answers whether [`persist_notes_root`] can write a folder, so a move it
/// could not record is turned down before any file moves.
pub(crate) fn check_notes_root_writable(state: &AppState, root: &str) -> Result<(), String> {
    if !writes_are_held(state) {
        return Ok(());
    }
    notes_root_in_file(state, root).map(|_| ())
}

/// config.toml's text as it stands with `notes.root` set to `root`.
fn notes_root_in_file(state: &AppState, root: &str) -> Result<String, String> {
    let text = state.config_store.read_text().map_err(|e| e.to_string())?;
    NotesConfig::write_root_into(&text, root).map_err(|error| {
        tracing::warn!(%error, "config.toml cannot take a new folder");
        FOLDER_CANNOT_BE_SAVED.to_string()
    })
}

/// Writes `contents` over config.toml, recording them in the watcher ignore
/// set first.
///
/// The key is the config file's canonical path under the config namespace,
/// which is what the config watcher looks up. The bare `config.toml` it used
/// to be was shared with every note of that name (ADR-028 section 6).
fn write_config_text(state: &AppState, contents: &str) -> Result<(), String> {
    let key = writ_core::watcher::ignore::config_key(&crate::watcher::handler::ignore_key_path(
        state.config_store.path(),
    ));

    {
        let mut ignore = recover_poison(
            state.watcher_ignore.lock(),
            "commands::config::write_config_text",
        );
        ignore.record(key, contents.as_bytes(), Instant::now());
    }

    state
        .config_store
        .write_serialized(contents)
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

/// Changes the live config with `change` and writes the result, all under
/// the config lock (TS-29).
///
/// `change` works on a copy and answers whether it changed anything; nothing
/// is written when it did not. The copy replaces the live config only once it
/// is on disk, so a failed write leaves both as they were. Holding the lock
/// across the write is what keeps two changes from losing one another: a
/// command, a frontend write and a reload of a hand edit each wait for the
/// one before to land, rather than each replacing the config with a copy read
/// before the other's change.
pub(crate) fn mutate_config<F>(state: &AppState, change: F) -> Result<bool, String>
where
    F: FnOnce(&mut WritConfig) -> bool,
{
    let mut live = recover_poison(state.config.lock(), "commands::config::mutate_config");
    let mut next = live.clone();
    if !change(&mut next) {
        return Ok(false);
    }
    persist_config(state, &next)?;
    *live = next;
    Ok(true)
}

/// [`update_config`] without the IPC wrapper or the menu bar.
pub fn update_config_inner(state: &AppState, config: WritConfig) -> Result<(), String> {
    mutate_config(state, |live| {
        merge_into_live(live, config);
        true
    })
    .map(|_| ())
}

/// IPC: takes the frontend's whole config, writes it, and draws the menu bar
/// again when the write turned an app on or off (ADR-042 section 3).
#[tauri::command]
pub fn update_config(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    config: WritConfig,
) -> Result<(), String> {
    update_config_inner(&state, config)?;
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
