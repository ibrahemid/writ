//! What the harness did, and which programs may reach the notes folder.
//!
//! Two surfaces over one record: the activity list the panel shows, and the
//! approvals the settings section edits. Approval is granted here and only
//! here — the command line writes none, and no protocol message can grant one
//! (ADR-031 rule 3.4). A change lands in `config.toml`, which the running
//! server re-reads on its next call, so revoking takes effect without a
//! restart (rule 3.5).
//!
//! Nothing in this module reads a note. The records it hands the panel carry a
//! tool name, a client name, a path and a length, and the record type has no
//! field that could hold anything else (rule 5.1).

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};
use writ_core::activity::ActivityRecord;
use writ_core::config::mcp::{ClientApproval, McpConfig};

use crate::events::emitter::{emit_event, WritFrontendEvent};
use crate::poison::recover_poison;
use crate::state::AppState;

/// Most records one call answers with, whatever the panel asked for.
///
/// The panel is a list a person reads, not an export, and the log is capped at
/// two generations either way.
pub const MAX_ACTIVITY_LIMIT: usize = 500;

/// Why an approval could not be changed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApprovalError {
    /// The name was empty or only spaces.
    ///
    /// A program with no name gives the user nothing to recognise later, and
    /// the gate answers it the same way whatever is recorded, so there is
    /// nothing to store.
    #[error("A program that sent no name cannot be approved.")]
    NoName,
}

/// The command a client is given, and the file it runs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct McpServerCommand {
    /// The `writ` binary, as an absolute path when one was found.
    pub path: String,
    /// The whole line to paste into a client's configuration.
    pub command: String,
}

// --- Activity ---------------------------------------------------------------

/// The newest records, newest first, held to [`MAX_ACTIVITY_LIMIT`].
pub fn activity_recent_inner(writ_dir: &Path, limit: usize) -> Vec<ActivityRecord> {
    writ_storage::activity_log::read_recent(writ_dir, limit.min(MAX_ACTIVITY_LIMIT))
}

/// Forgets both generations of the log.
pub fn activity_clear_inner(writ_dir: &Path) -> Result<(), String> {
    writ_storage::activity_log::clear(writ_dir).map_err(|error| error.to_string())
}

/// IPC: the newest activity records.
#[tauri::command]
pub fn activity_recent(app: AppHandle, limit: usize) -> Vec<ActivityRecord> {
    let state = app.state::<AppState>();
    activity_recent_inner(&state.writ_dir, limit)
}

/// IPC: forget everything the log holds.
#[tauri::command]
pub fn activity_clear(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    activity_clear_inner(&state.writ_dir)?;
    // The panel asked for this, but a second view of the same log has not; the
    // event is what tells it the list is now empty.
    if let Err(error) = emit_event(&app, WritFrontendEvent::ActivityChanged {}) {
        tracing::warn!(error = %error, "failed to emit activity event");
    }
    Ok(())
}

// --- Approvals --------------------------------------------------------------

/// The approvals as the settings section lists them, by name.
pub fn mcp_clients_inner(config: &McpConfig) -> Vec<ClientApproval> {
    let mut clients = config.approved_clients.clone();
    clients.sort_by(|a, b| a.name.cmp(&b.name));
    clients
}

/// Sets what one client may do, adding it to the list when it is not there yet.
///
/// The name is stored exactly as the client sent it, because that is the string
/// the gate matches (`McpConfig::approval_for`). Granting neither direction is
/// a legal state and is not the same as forgetting: the client stays on the
/// list, so it is not offered again as something new to decide on.
pub fn set_client_permission_inner(
    config: &mut McpConfig,
    name: &str,
    read: bool,
    write: bool,
) -> Result<(), ApprovalError> {
    if name.trim().is_empty() {
        return Err(ApprovalError::NoName);
    }
    match config
        .approved_clients
        .iter_mut()
        .find(|approval| approval.name == name)
    {
        Some(approval) => {
            approval.read = read;
            approval.write = write;
        }
        None => config.approved_clients.push(ClientApproval {
            name: name.to_string(),
            first_seen: chrono::Utc::now(),
            read,
            write,
        }),
    }
    Ok(())
}

/// Takes a client off the list, so its next call is pending again.
///
/// A name that is not on the list is already forgotten, so this answers the
/// same way rather than failing.
pub fn forget_client_inner(config: &mut McpConfig, name: &str) {
    config
        .approved_clients
        .retain(|approval| approval.name != name);
}

/// IPC: the clients the user has decided on.
#[tauri::command]
pub fn mcp_clients(app: AppHandle) -> Vec<ClientApproval> {
    let state = app.state::<AppState>();
    let guard = recover_poison(state.config.lock(), "commands::activity::mcp_clients");
    mcp_clients_inner(&guard.mcp)
}

/// IPC: set what one client may do.
#[tauri::command]
pub fn mcp_set_client_permission(
    app: AppHandle,
    name: String,
    read: bool,
    write: bool,
) -> Result<Vec<ClientApproval>, String> {
    write_approvals(&app, "mcp_set_client_permission", |mcp| {
        set_client_permission_inner(mcp, &name, read, write).map_err(|e| e.to_string())
    })
}

/// IPC: forget one client.
#[tauri::command]
pub fn mcp_forget_client(app: AppHandle, name: String) -> Result<Vec<ClientApproval>, String> {
    write_approvals(&app, "mcp_forget_client", |mcp| {
        forget_client_inner(mcp, &name);
        Ok(())
    })
}

/// Applies `change` to the `[mcp]` section under the lock and persists it.
///
/// The section is edited inside the lock and re-read there, so a settings write
/// landing between a read and this point is not overwritten with a stale clone.
/// On a failed disk write the list is put back, so memory and disk never
/// disagree about who was approved — the same shape `ai_consent_host` holds.
fn write_approvals(
    app: &AppHandle,
    location: &'static str,
    change: impl FnOnce(&mut McpConfig) -> Result<(), String>,
) -> Result<Vec<ClientApproval>, String> {
    let state = app.state::<AppState>();
    let (updated, previous) = {
        let mut guard = recover_poison(state.config.lock(), location);
        let previous = guard.mcp.approved_clients.clone();
        change(&mut guard.mcp)?;
        (guard.clone(), previous)
    };
    if let Err(reason) = super::config::persist_config(&state, &updated) {
        let mut guard = recover_poison(state.config.lock(), location);
        guard.mcp.approved_clients = previous;
        return Err(reason);
    }
    // `persist_config` records the write in the watcher's ignore set, so the
    // file change never comes back as external and the frontend's copy of the
    // settings would keep the old list. The next settings edit sends that copy
    // whole, which would drop the approval just written.
    if let Err(error) = emit_event(
        app,
        WritFrontendEvent::ConfigChanged {
            keys: vec!["mcp".to_string()],
        },
    ) {
        tracing::warn!(error = %error, "failed to emit config event");
    }
    Ok(mcp_clients_inner(&updated.mcp))
}

// --- The command a client is given ------------------------------------------

/// The verb the server is served under.
const SERVE_VERB: &str = "mcp";

/// The command line for `binary`.
///
/// Quoted, because an app installed under a folder with a space in its name is
/// the normal case on macOS. A client launched from the desktop inherits a
/// short PATH, so [`resolved_cli_path`] hands this a full path wherever one is
/// known; on Windows the installer puts `writ.exe` on the PATH itself and the
/// bare name is what resolves.
pub fn server_command_for(binary: &Path) -> McpServerCommand {
    let path = binary.display().to_string();
    McpServerCommand {
        command: format!("\"{path}\" {SERVE_VERB}"),
        path,
    }
}

/// IPC: the command to paste into a client's configuration.
#[tauri::command]
pub fn mcp_server_command() -> McpServerCommand {
    server_command_for(&resolved_cli_path())
}

/// The `writ` binary this app would have a client launch.
///
/// The copy bundled beside the app executable is preferred: it is the build
/// that matches the running app, and it is there whether or not the user ever
/// asked for the command to be linked onto their PATH. The linked copy is the
/// fallback for a development run, where nothing is bundled.
fn resolved_cli_path() -> PathBuf {
    super::cli::bundled_cli_path()
        .unwrap_or_else(|| PathBuf::from(super::cli::path_command_fallback()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approved(name: &str, read: bool, write: bool) -> ClientApproval {
        ClientApproval {
            name: name.to_string(),
            first_seen: chrono::Utc::now(),
            read,
            write,
        }
    }

    #[test]
    fn clients_are_listed_by_name() {
        let config = McpConfig {
            enabled: true,
            approved_clients: vec![
                approved("Zed", true, false),
                approved("Claude Code", true, true),
            ],
        };
        let names: Vec<String> = mcp_clients_inner(&config)
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, ["Claude Code", "Zed"]);
    }

    #[test]
    fn a_client_that_was_not_on_the_list_is_added_with_the_time_it_was_decided_on() {
        let mut config = McpConfig::default();
        set_client_permission_inner(&mut config, "Claude Code", true, false).expect("set");

        let approval = config.approval_for("Claude Code").expect("the client");
        assert!(approval.read);
        assert!(!approval.write);
        assert!(approval.first_seen > chrono::DateTime::<chrono::Utc>::UNIX_EPOCH);
    }

    #[test]
    fn setting_one_client_leaves_the_others_alone() {
        let mut config = McpConfig {
            enabled: true,
            approved_clients: vec![
                approved("Claude Code", true, false),
                approved("Zed", true, true),
            ],
        };
        set_client_permission_inner(&mut config, "Claude Code", true, true).expect("set");

        assert!(config.approval_for("Claude Code").expect("client").write);
        assert!(config.approval_for("Zed").expect("client").read);
        assert_eq!(config.approved_clients.len(), 2);
    }

    #[test]
    fn granting_neither_direction_keeps_the_client_on_the_list() {
        let mut config = McpConfig::default();
        set_client_permission_inner(&mut config, "Claude Code", true, true).expect("set");
        set_client_permission_inner(&mut config, "Claude Code", false, false).expect("set");

        let approval = config.approval_for("Claude Code").expect("the client");
        assert!(!approval.read);
        assert!(!approval.write);
    }

    #[test]
    fn a_program_that_sent_no_name_cannot_be_approved() {
        let mut config = McpConfig::default();
        for name in ["", "   "] {
            assert_eq!(
                set_client_permission_inner(&mut config, name, true, false),
                Err(ApprovalError::NoName),
                "{name:?}"
            );
        }
        assert!(config.approved_clients.is_empty());
    }

    #[test]
    fn forgetting_takes_the_client_off_the_list() {
        let mut config = McpConfig {
            enabled: true,
            approved_clients: vec![
                approved("Claude Code", true, true),
                approved("Zed", true, false),
            ],
        };
        forget_client_inner(&mut config, "Claude Code");

        assert!(config.approval_for("Claude Code").is_none());
        assert!(config.approval_for("Zed").is_some());
    }

    #[test]
    fn forgetting_a_client_that_is_not_on_the_list_changes_nothing() {
        let mut config = McpConfig {
            enabled: true,
            approved_clients: vec![approved("Zed", true, false)],
        };
        forget_client_inner(&mut config, "Claude Code");

        assert_eq!(config.approved_clients.len(), 1);
    }

    #[test]
    fn the_command_names_the_binary_and_the_verb() {
        let command = server_command_for(Path::new("/Applications/Writ.app/Contents/MacOS/writ"));
        assert_eq!(command.path, "/Applications/Writ.app/Contents/MacOS/writ");
        assert_eq!(
            command.command,
            "\"/Applications/Writ.app/Contents/MacOS/writ\" mcp"
        );
    }

    #[test]
    fn a_path_with_a_space_is_quoted() {
        let command = server_command_for(Path::new("/Users/a b/Writ.app/Contents/MacOS/writ"));
        assert!(command.command.starts_with('"'));
        assert!(command.command.ends_with("\" mcp"));
    }

    #[test]
    fn the_fallback_is_the_linked_command_off_windows_and_the_path_name_on_it() {
        let fallback = super::super::cli::path_command_fallback();
        if cfg!(windows) {
            assert_eq!(fallback, "writ.exe");
        } else {
            assert_eq!(fallback, "/usr/local/bin/writ");
        }
    }

    #[test]
    fn a_build_that_bundles_nothing_still_names_a_command() {
        // Under `cargo test` there is no bundled binary beside the test
        // executable, so this is the fallback path being selected.
        let command = server_command_for(&resolved_cli_path());
        assert!(command.command.ends_with("\" mcp"));
        assert!(!command.path.is_empty());
    }
}
