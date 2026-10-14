//! IPC for how often each palette command ran (ADR-048). The counts live in
//! writ.db ([`writ_storage::command_usage`]), so running a command writes a
//! row there and never touches config.toml.

use std::collections::{HashMap, HashSet};

use tauri::State;
use writ_core::command::usage::{CommandUsage, CommandUse};

use crate::state::AppState;

/// Longest command id a write accepts. Writ's own ids are a few dozen bytes.
const MAX_COMMAND_ID_LEN: usize = 256;

fn check_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > MAX_COMMAND_ID_LEN {
        return Err(format!(
            "command id must be 1 to {MAX_COMMAND_ID_LEN} bytes, got {}",
            id.len()
        ));
    }
    Ok(())
}

/// [`get_command_usage`] without the IPC wrapper.
pub fn get_command_usage_inner(state: &AppState) -> Result<HashMap<String, CommandUsage>, String> {
    state.command_usage.all().map_err(|e| e.to_string())
}

/// IPC: every command's usage, keyed by command id.
#[tauri::command]
pub fn get_command_usage(
    state: State<'_, AppState>,
) -> Result<HashMap<String, CommandUsage>, String> {
    get_command_usage_inner(&state)
}

/// [`record_command_uses`] without the IPC wrapper.
pub fn record_command_uses_inner(state: &AppState, uses: &[CommandUse]) -> Result<(), String> {
    for item in uses {
        check_id(&item.id)?;
    }
    state.command_usage.record(uses).map_err(|e| e.to_string())
}

/// IPC: adds the runs the palette batched up since its last write.
#[tauri::command]
pub fn record_command_uses(
    state: State<'_, AppState>,
    uses: Vec<CommandUse>,
) -> Result<(), String> {
    record_command_uses_inner(&state, &uses)
}

/// [`clear_command_usage`] without the IPC wrapper.
pub fn clear_command_usage_inner(state: &AppState) -> Result<(), String> {
    state.command_usage.clear().map_err(|e| e.to_string())
}

/// IPC: forgets every command's usage.
#[tauri::command]
pub fn clear_command_usage(state: State<'_, AppState>) -> Result<(), String> {
    clear_command_usage_inner(&state)
}

/// [`prune_command_usage`] without the IPC wrapper.
pub fn prune_command_usage_inner(state: &AppState, known_ids: Vec<String>) -> Result<(), String> {
    let known: HashSet<String> = known_ids.into_iter().collect();
    state
        .command_usage
        .prune(&known)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// IPC: drops the usage of commands that no longer exist.
#[tauri::command]
pub fn prune_command_usage(
    state: State<'_, AppState>,
    known_ids: Vec<String>,
) -> Result<(), String> {
    prune_command_usage_inner(&state, known_ids)
}
