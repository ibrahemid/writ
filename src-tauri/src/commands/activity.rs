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

use std::path::Path;
use std::time::Instant;

use tauri::{AppHandle, Manager};
use writ_core::activity::{ActivityRecord, PendingClient};
use writ_core::cli_install::{client_command, ClientCommandFacts, InstallTarget};
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

/// Every program the harness knows about.
///
/// Two lists rather than one, because the two are not the same thing: an
/// approval is a decision the user made and lives in `config.toml`, while a
/// waiting program is one the harness saw and nobody has decided on, held in
/// `mcp-pending.json` so no volume of calls can push it out of the capped log.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct McpClients {
    /// Decided on, in either direction or neither, sorted by name.
    pub approved: Vec<ClientApproval>,
    /// Seen and not decided on, oldest first seen first.
    pub waiting: Vec<PendingClient>,
}

/// The command a client is given, and the file it runs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct McpServerCommand {
    /// The `writ` binary, as an absolute path when one was found.
    pub path: String,
    /// The whole line to paste into a client's configuration.
    pub command: String,
    /// The command is the copy the install puts on the PATH, and it is not
    /// there yet: an AppImage's own copy is gone once it quits.
    pub needs_install: bool,
    /// The app runs from a folder macOS made up for a quarantined download,
    /// and a client handed that path loses it at the next launch.
    pub needs_move: bool,
}

/// What an approved program can do, split the way the two grants are.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct McpTools {
    /// The tools the read grant covers.
    pub read: Vec<String>,
    /// The tools the write grant covers.
    pub write: Vec<String>,
    /// The tools the open-file grant covers (ADR-053).
    pub context: Vec<String>,
}

/// IPC: the tools an approved program can call.
///
/// Read from `writ_core::tools`, the same list the server registers and the
/// gate splits on, so the settings row cannot promise a tool that is not there
/// or hide one that is. The app does not depend on `writ-mcp` for this: the
/// names are policy and live in core.
#[tauri::command]
pub fn mcp_tools() -> McpTools {
    McpTools {
        read: writ_core::tools::READ_TOOLS
            .iter()
            .map(|name| name.to_string())
            .collect(),
        write: writ_core::tools::WRITE_TOOLS
            .iter()
            .map(|name| name.to_string())
            .collect(),
        context: writ_core::tools::CONTEXT_TOOLS
            .iter()
            .map(|name| name.to_string())
            .collect(),
    }
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
/// the gate matches (`McpConfig::approval_for`). Writing is stored with reading,
/// as the settings load it. Granting neither direction is a legal state and is
/// not the same as forgetting: it is the person's "Don't allow", and the client
/// stays on the list, so it is not offered again as something new to decide on.
pub fn set_client_permission_inner(
    config: &mut McpConfig,
    name: &str,
    read: bool,
    write: bool,
) -> Result<(), ApprovalError> {
    if name.trim().is_empty() {
        return Err(ApprovalError::NoName);
    }
    let read = read || write;
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
            context: false,
            review: false,
        }),
    }
    Ok(())
}

/// Sets whether one client may see the file open in Writ and its selection
/// (ADR-053), adding it to the list when it is not there yet.
///
/// Its own grant: reading and writing stay as they were, and a client added
/// here reads and writes nothing until the person says so.
pub fn set_client_context_inner(
    config: &mut McpConfig,
    name: &str,
    context: bool,
) -> Result<(), ApprovalError> {
    if name.trim().is_empty() {
        return Err(ApprovalError::NoName);
    }
    match config
        .approved_clients
        .iter_mut()
        .find(|approval| approval.name == name)
    {
        Some(approval) => approval.context = context,
        None => config.approved_clients.push(ClientApproval {
            name: name.to_string(),
            first_seen: chrono::Utc::now(),
            read: false,
            write: false,
            context,
            review: false,
        }),
    }
    Ok(())
}

/// Sets whether each of one client's writes waits for the person to apply or
/// discard it in Writ (ADR-053), adding the client when it is not there yet.
///
/// Its own setting: reading, writing and the open file stay as they were. It
/// is read only while the client may write, so a client added here writes
/// nothing until the person says so.
pub fn set_client_review_inner(
    config: &mut McpConfig,
    name: &str,
    review: bool,
) -> Result<(), ApprovalError> {
    if name.trim().is_empty() {
        return Err(ApprovalError::NoName);
    }
    match config
        .approved_clients
        .iter_mut()
        .find(|approval| approval.name == name)
    {
        Some(approval) => approval.review = review,
        None => config.approved_clients.push(ClientApproval {
            name: name.to_string(),
            first_seen: chrono::Utc::now(),
            read: false,
            write: false,
            context: false,
            review,
        }),
    }
    Ok(())
}

pub fn deny_client_inner(config: &mut McpConfig, name: &str) -> Result<(), ApprovalError> {
    set_client_permission_inner(config, name, false, false)?;
    set_client_context_inner(config, name, false)
}

pub fn undo_denial_inner(config: &mut McpConfig, name: &str) -> Option<ClientApproval> {
    let index = config
        .approved_clients
        .iter()
        .position(|approval| approval.name == name && approval.grants_nothing())?;
    Some(config.approved_clients.remove(index))
}

pub fn restore_client_inner(
    config: &mut McpConfig,
    approval: ClientApproval,
) -> Result<(), ApprovalError> {
    if approval.name.trim().is_empty() {
        return Err(ApprovalError::NoName);
    }
    let approval = ClientApproval {
        read: approval.read || approval.write,
        ..approval
    };
    match config
        .approved_clients
        .iter_mut()
        .find(|existing| existing.name == approval.name)
    {
        Some(existing) => *existing = approval,
        None => config.approved_clients.push(approval),
    }
    Ok(())
}

/// The denial's own time stands in for when the program first asked: the
/// server's entry was dropped when the decision was made.
pub fn waiting_again(approval: &ClientApproval) -> PendingClient {
    PendingClient {
        name: approval.name.clone(),
        version: None,
        first_seen: approval.first_seen,
        last_seen: approval.first_seen,
        calls: 0,
    }
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

/// IPC: every program the harness knows about.
#[tauri::command]
pub fn mcp_clients(app: AppHandle) -> McpClients {
    let state = app.state::<AppState>();
    let approved = {
        let guard = recover_poison(state.config.lock(), "commands::activity::mcp_clients");
        mcp_clients_inner(&guard.mcp)
    };
    McpClients {
        waiting: waiting_in(&state.writ_dir, &approved),
        approved,
    }
}

/// Who is waiting, minus anyone already decided on.
///
/// A program can be in both files for one call: the server writes the waiting
/// entry, the user approves it in the app, and the entry is dropped there. If a
/// write fails on the way, the approval is what counts.
pub fn waiting_in(writ_dir: &Path, approved: &[ClientApproval]) -> Vec<PendingClient> {
    undecided(writ_storage::pending_clients::read(writ_dir), approved)
}

pub fn waiting_settled_in(
    writ_dir: &Path,
    approved: &[ClientApproval],
) -> Option<Vec<PendingClient>> {
    writ_storage::pending_clients::read_settled(writ_dir)
        .map(|waiting| undecided(waiting, approved))
}

fn undecided(waiting: Vec<PendingClient>, approved: &[ClientApproval]) -> Vec<PendingClient> {
    let decided: std::collections::HashSet<&str> = approved
        .iter()
        .map(|approval| approval.name.as_str())
        .collect();
    waiting
        .into_iter()
        .filter(|entry| !decided.contains(entry.name.as_str()))
        .collect()
}

pub fn announce_waiting(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Some(event) = announce_waiting_inner(&state) else {
        tracing::debug!("the waiting file could not be read whole; skipped");
        return;
    };
    if let Err(error) = emit_event(app, event) {
        tracing::warn!(error = %error, "failed to emit the waiting programs");
    }
}

/// A file caught mid-write sends nothing: the write that completes it raises
/// its own change, and an empty list in between would hide every waiting
/// program.
pub fn announce_waiting_inner(state: &AppState) -> Option<WritFrontendEvent> {
    let approved = {
        let guard = recover_poison(state.config.lock(), "commands::activity::announce_waiting");
        mcp_clients_inner(&guard.mcp)
    };
    waiting_settled_in(&state.writ_dir, &approved)
        .map(|waiting| WritFrontendEvent::McpPending { waiting })
}

/// What the app hands `pending_clients` before it writes the waiting file:
/// records the bytes in the watcher's ignore set, so the app's own write does
/// not come back to it as the server's (the watcher rule).
fn stamp_waiting_write(state: &AppState) -> impl FnOnce(&[u8]) + '_ {
    move |bytes| {
        let key = writ_core::watcher::ignore::data_key(&crate::watcher::handler::ignore_key_path(
            &writ_storage::pending_clients::path(&state.writ_dir),
        ));
        let mut ignore = recover_poison(
            state.watcher_ignore.lock(),
            "commands::activity::stamp_waiting_write",
        );
        ignore.record(key, bytes, Instant::now());
    }
}

/// Whether setting `name` to `write` hands it writing it does not hold yet.
///
/// Writing changes, creates and renames notes, so it is the one direction the
/// person confirms outside the webview (ADR-031 rule 3.8). Taking writing away,
/// or keeping a grant already made, asks nothing.
pub fn is_new_write_grant(config: &McpConfig, name: &str, write: bool) -> bool {
    write
        && !config
            .approved_clients
            .iter()
            .any(|approval| approval.name == name && approval.write)
}

/// The longest program name the confirmation shows before it is cut.
const DIALOG_NAME_MAX_CHARS: usize = 60;

/// A program's name as the confirmation shows it.
///
/// The name is whatever the program sent, so it is cut to one short line:
/// control characters become spaces and a long name ends in an ellipsis, and
/// no name can add lines of its own to the dialog's text.
pub fn dialog_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.chars().count() <= DIALOG_NAME_MAX_CHARS {
        return cleaned.to_string();
    }
    let cut: String = cleaned.chars().take(DIALOG_NAME_MAX_CHARS).collect();
    format!("{}…", cut.trim_end())
}

/// Asks the person in a native dialog whether `name` may change their files.
///
/// The webview can call the grant commands, so a grant of writing is confirmed
/// where no script in it can draw or answer the question. Waits on the dialog,
/// so it is only called from an async command, never on the main thread.
fn confirm_write_grant(app: &AppHandle, name: &str) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    app.dialog()
        .message("It can write, create and rename files in your folder.")
        .title(format!(
            "Let \u{201c}{}\u{201d} change your files?",
            dialog_name(name)
        ))
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Allow".to_string(),
            "Cancel".to_string(),
        ))
        .show(move |allowed| {
            let _ = tx.send(allowed);
        });
    rx.recv().unwrap_or(false)
}

/// Whether the grant may go ahead: it hands out no new writing, or the person
/// allowed it. The settings lock is let go before the dialog is up.
fn write_grant_allowed(app: &AppHandle, location: &'static str, name: &str, write: bool) -> bool {
    let is_new = {
        let state = app.state::<AppState>();
        let guard = recover_poison(state.config.lock(), location);
        is_new_write_grant(&guard.mcp, name, write)
    };
    !is_new || confirm_write_grant(app, name)
}

/// IPC: set what one client may do.
///
/// A grant of writing waits on the native confirmation; turned down, nothing
/// changes and the lists come back as they were.
#[tauri::command]
pub async fn mcp_set_client_permission(
    app: AppHandle,
    name: String,
    read: bool,
    write: bool,
) -> Result<McpClients, String> {
    let location = "commands::activity::mcp_set_client_permission";
    if !write_grant_allowed(&app, location, &name, write) {
        return Ok(clients_now(&app.state::<AppState>(), location));
    }
    write_approvals(&app, "mcp_set_client_permission", &name, |mcp| {
        set_client_permission_inner(mcp, &name, read, write).map_err(|e| e.to_string())
    })
}

/// IPC: set whether one client may see the file open in Writ (ADR-053).
///
/// Asks nothing outside the webview, as a grant of reading does not: rule 3.8
/// confirms writing alone.
#[tauri::command]
pub fn mcp_set_client_context(
    app: AppHandle,
    name: String,
    context: bool,
) -> Result<McpClients, String> {
    write_approvals(&app, "mcp_set_client_context", &name, |mcp| {
        set_client_context_inner(mcp, &name, context).map_err(|e| e.to_string())
    })
}

/// IPC: set whether each of one client's writes waits for the person's answer
/// in Writ (ADR-053).
///
/// Asks nothing outside the webview: the setting holds writes back and never
/// lets one through that the grant of writing did not already allow.
#[tauri::command]
pub fn mcp_set_client_review(
    app: AppHandle,
    name: String,
    review: bool,
) -> Result<McpClients, String> {
    write_approvals(&app, "mcp_set_client_review", &name, |mcp| {
        set_client_review_inner(mcp, &name, review).map_err(|e| e.to_string())
    })
}

/// IPC: "Don't allow" one program.
///
/// Stored as an entry granting neither direction, so a program that keeps
/// calling is answered as not allowed and stays off the waiting list. A program
/// that sent no name cannot be stored, and the gate refuses it whatever is on
/// the list, so its waiting entry is all there is to take away.
#[tauri::command]
pub fn mcp_deny_client(app: AppHandle, name: String) -> Result<McpClients, String> {
    if name.trim().is_empty() {
        return deny_unnamed_inner(&app.state::<AppState>(), &name);
    }
    write_approvals(&app, "mcp_deny_client", &name, |mcp| {
        deny_client_inner(mcp, &name).map_err(|e| e.to_string())
    })
}

pub fn deny_unnamed_inner(state: &AppState, name: &str) -> Result<McpClients, String> {
    writ_storage::pending_clients::forget(&state.writ_dir, name, stamp_waiting_write(state))
        .map_err(|error| error.to_string())?;
    Ok(clients_now(state, "commands::activity::mcp_deny_client"))
}

/// IPC: undo "Don't allow", putting the program back on the waiting list.
///
/// Answers the lists as they are when there was no denial to undo.
#[tauri::command]
pub fn mcp_return_to_waiting(app: AppHandle, name: String) -> Result<McpClients, String> {
    let state = app.state::<AppState>();
    return_to_waiting_inner(&state, &name, || tell_settings_moved(&app))
}

pub fn return_to_waiting_inner(
    state: &AppState,
    name: &str,
    settings_moved: impl FnOnce(),
) -> Result<McpClients, String> {
    let location = "commands::activity::mcp_return_to_waiting";
    let (updated, undone, previous) = {
        let mut guard = recover_poison(state.config.lock(), location);
        let previous = guard.mcp.approved_clients.clone();
        let undone = undo_denial_inner(&mut guard.mcp, name);
        (guard.clone(), undone, previous)
    };
    let Some(undone) = undone else {
        return Ok(clients_now(state, location));
    };
    if let Err(reason) = super::config::persist_config(state, &updated) {
        let mut guard = recover_poison(state.config.lock(), location);
        guard.mcp.approved_clients = previous;
        return Err(reason);
    }
    settings_moved();
    writ_storage::pending_clients::put_back(
        &state.writ_dir,
        waiting_again(&undone),
        stamp_waiting_write(state),
    )
    .map_err(|error| error.to_string())?;
    Ok(clients_now(state, location))
}

/// IPC: undo a forget, putting the approval back as it was.
///
/// The approval comes from the webview, so one that carries writing the
/// program no longer holds waits on the same native confirmation a grant does.
#[tauri::command]
pub async fn mcp_restore_client(
    app: AppHandle,
    approval: ClientApproval,
) -> Result<McpClients, String> {
    let location = "commands::activity::mcp_restore_client";
    let name = approval.name.clone();
    if !write_grant_allowed(&app, location, &name, approval.write) {
        return Ok(clients_now(&app.state::<AppState>(), location));
    }
    write_approvals(&app, "mcp_restore_client", &name, |mcp| {
        restore_client_inner(mcp, approval).map_err(|e| e.to_string())
    })
}

fn clients_now(state: &AppState, location: &'static str) -> McpClients {
    let approved = {
        let guard = recover_poison(state.config.lock(), location);
        mcp_clients_inner(&guard.mcp)
    };
    McpClients {
        waiting: waiting_in(&state.writ_dir, &approved),
        approved,
    }
}

/// Tells the frontend the settings it holds moved. `persist_config` records the
/// write in the watcher's ignore set, so the file change never comes back as
/// external and the frontend's copy of the settings would keep the old list.
/// The next settings edit sends that copy whole, which would drop the approval
/// just written.
fn tell_settings_moved(app: &AppHandle) {
    if let Err(error) = emit_event(
        app,
        WritFrontendEvent::ConfigChanged {
            keys: vec!["mcp".to_string()],
        },
    ) {
        tracing::warn!(error = %error, "failed to emit config event");
    }
}

/// IPC: forget one client.
#[tauri::command]
pub fn mcp_forget_client(app: AppHandle, name: String) -> Result<McpClients, String> {
    write_approvals(&app, "mcp_forget_client", &name, |mcp| {
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
    name: &str,
    change: impl FnOnce(&mut McpConfig) -> Result<(), String>,
) -> Result<McpClients, String> {
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
    tell_settings_moved(app);

    // The decision is made, so the program is no longer one to decide on. A
    // failed write leaves a stale entry, which the filter in `waiting_in` hides
    // anyway once the name is on the approved list.
    if let Err(error) =
        writ_storage::pending_clients::forget(&state.writ_dir, name, stamp_waiting_write(&state))
    {
        tracing::warn!(error = %error, "failed to clear the waiting client");
    }

    let approved = mcp_clients_inner(&updated.mcp);
    Ok(McpClients {
        waiting: waiting_in(&state.writ_dir, &approved),
        approved,
    })
}

// --- The command a client is given ------------------------------------------

/// The verb the server is served under.
const SERVE_VERB: &str = "mcp";

/// The command line for `binary`.
///
/// Quoted, because an app installed under a folder with a space in its name is
/// the normal case on macOS. A client launched from the desktop inherits a
/// short PATH, so [`resolved_command`] hands this a full path wherever one is
/// known; on Windows the installer puts `writ.exe` on the PATH itself and the
/// bare name is what resolves.
pub fn server_command_for(binary: &Path) -> McpServerCommand {
    let path = binary.display().to_string();
    McpServerCommand {
        command: format!("\"{path}\" {SERVE_VERB}"),
        path,
        needs_install: false,
        needs_move: false,
    }
}

/// IPC: the command to paste into a client's configuration, off the main
/// thread: finding it reads the folder the app runs from.
#[tauri::command]
pub async fn mcp_server_command() -> Result<McpServerCommand, String> {
    super::offload::run_detached(super::offload::ERR_COMMAND_STOPPED, || {
        Ok(resolved_command())
    })
    .await
}

/// The `writ` this app would have a client launch
/// ([`writ_core::cli_install::client_command`]): the copy bundled with the
/// running build, the installed copy for an AppImage whose own copy is gone
/// once it quits, and the install target for a development run.
pub(crate) fn resolved_command() -> McpServerCommand {
    let target = super::cli::install_target();
    let exe = std::env::current_exe().ok();
    let bundled = super::cli::bundled_cli_path();
    let installed = match &target {
        Some(InstallTarget::Copy(path) | InstallTarget::Link(path)) => path.metadata().is_ok(),
        Some(InstallTarget::ByInstaller) | None => false,
    };
    let client = client_command(ClientCommandFacts {
        exe: exe.as_deref(),
        bundled: bundled.as_deref(),
        target: target.as_ref(),
        installed,
    });
    McpServerCommand {
        needs_install: client.needs_install,
        needs_move: client.needs_move,
        ..server_command_for(&client.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use writ_core::activity::ClientId;

    fn approved(name: &str, read: bool, write: bool) -> ClientApproval {
        ClientApproval {
            name: name.to_string(),
            first_seen: chrono::Utc::now(),
            read,
            write,
            context: false,
            review: false,
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
    fn writing_is_stored_with_reading() {
        let mut config = McpConfig::default();
        set_client_permission_inner(&mut config, "Claude Code", false, true).expect("set");

        let approval = config.approval_for("Claude Code").expect("the client");
        assert!(approval.read && approval.write);
    }

    #[test]
    fn not_allowing_a_program_stores_a_decision_that_grants_nothing() {
        let mut config = McpConfig::default();
        deny_client_inner(&mut config, "Claude Code").expect("deny");

        let approval = config
            .approval_for("Claude Code")
            .expect("the client is listed");
        assert!(approval.grants_nothing());
    }

    #[test]
    fn seeing_the_open_file_is_set_on_its_own_and_kept_by_the_other_grants() {
        let mut config = McpConfig::default();
        set_client_permission_inner(&mut config, "Claude Code", true, false).expect("read");
        set_client_context_inner(&mut config, "Claude Code", true).expect("context");

        let approval = config.approval_for("Claude Code").expect("the client");
        assert!(approval.read && approval.context && !approval.write);

        set_client_permission_inner(&mut config, "Claude Code", true, true).expect("write");
        assert!(
            config
                .approval_for("Claude Code")
                .expect("the client")
                .context
        );

        set_client_context_inner(&mut config, "Claude Code", false).expect("off");
        let approval = config.approval_for("Claude Code").expect("the client");
        assert!(approval.read && approval.write && !approval.context);
    }

    #[test]
    fn asking_before_each_write_is_set_on_its_own_and_holds_only_a_program_that_writes() {
        let mut config = McpConfig::default();
        set_client_permission_inner(&mut config, "Claude Code", true, true).expect("write");
        set_client_review_inner(&mut config, "Claude Code", true).expect("review");

        let approval = config.approval_for("Claude Code").expect("the client");
        assert!(approval.read && approval.write && approval.review);
        assert!(approval.reviews_writes());

        set_client_permission_inner(&mut config, "Claude Code", true, false).expect("read only");
        let approval = config.approval_for("Claude Code").expect("the client");
        assert!(approval.review && !approval.reviews_writes());

        set_client_review_inner(&mut config, "Codex", true).expect("added");
        let added = config.approval_for("Codex").expect("the client");
        assert!(!added.read && !added.write && !added.reviews_writes());
        assert_eq!(
            set_client_review_inner(&mut config, " ", true),
            Err(ApprovalError::NoName)
        );
    }

    #[test]
    fn a_program_let_see_the_open_file_alone_reads_and_writes_nothing() {
        let mut config = McpConfig::default();
        set_client_context_inner(&mut config, "Codex", true).expect("context");

        let approval = config.approval_for("Codex").expect("the client");
        assert!(approval.context && !approval.read && !approval.write);
        assert!(!approval.grants_nothing());
        assert_eq!(
            set_client_context_inner(&mut config, " ", true),
            Err(ApprovalError::NoName)
        );
    }

    #[test]
    fn not_allowing_a_program_takes_away_the_open_file_too() {
        let mut config = McpConfig::default();
        set_client_context_inner(&mut config, "Codex", true).expect("context");
        deny_client_inner(&mut config, "Codex").expect("deny");
        assert!(config
            .approval_for("Codex")
            .expect("the client")
            .grants_nothing());
    }

    #[test]
    fn the_tools_are_listed_by_the_grant_that_covers_them() {
        let tools = mcp_tools();
        assert_eq!(tools.context, vec!["get_context".to_string()]);
        assert!(tools.read.contains(&"reveal".to_string()));
        assert!(!tools.read.contains(&"get_context".to_string()));
    }

    #[test]
    fn not_allowing_a_program_that_sent_no_name_stores_nothing() {
        let mut config = McpConfig::default();
        assert_eq!(
            deny_client_inner(&mut config, "  "),
            Err(ApprovalError::NoName)
        );
        assert!(config.approved_clients.is_empty());
    }

    #[test]
    fn undoing_a_denial_takes_it_off_the_list_and_answers_it() {
        let mut config = McpConfig::default();
        deny_client_inner(&mut config, "Claude Code").expect("deny");
        set_client_permission_inner(&mut config, "Zed", true, false).expect("set");

        let undone = undo_denial_inner(&mut config, "Claude Code").expect("a denial");

        assert_eq!(undone.name, "Claude Code");
        assert!(config.approval_for("Claude Code").is_none());
        assert!(config.approval_for("Zed").is_some());
    }

    #[test]
    fn undoing_a_denial_leaves_a_program_that_was_since_allowed_alone() {
        let mut config = McpConfig::default();
        set_client_permission_inner(&mut config, "Claude Code", true, false).expect("set");

        assert_eq!(undo_denial_inner(&mut config, "Claude Code"), None);
        assert!(
            config
                .approval_for("Claude Code")
                .expect("still there")
                .read
        );
    }

    #[test]
    fn an_undone_denial_waits_again_from_when_it_was_decided() {
        let mut config = McpConfig::default();
        deny_client_inner(&mut config, "Claude Code").expect("deny");
        let denial = config
            .approval_for("Claude Code")
            .expect("the client")
            .clone();

        let waiting = waiting_again(&denial);

        assert_eq!(waiting.name, "Claude Code");
        assert_eq!(waiting.first_seen, denial.first_seen);
        assert_eq!(waiting.calls, 0);
    }

    #[test]
    fn a_forgotten_program_is_put_back_exactly_as_it_was() {
        let mut config = McpConfig {
            enabled: true,
            approved_clients: vec![approved("Claude Code", true, true)],
        };
        let before = config.approved_clients[0].clone();
        forget_client_inner(&mut config, "Claude Code");

        restore_client_inner(&mut config, before.clone()).expect("restore");

        assert_eq!(config.approved_clients, [before]);
    }

    #[test]
    fn restoring_replaces_what_is_on_the_list_under_the_name_now() {
        let mut config = McpConfig {
            enabled: true,
            approved_clients: vec![approved("Claude Code", false, false)],
        };
        let earlier = approved("Claude Code", true, false);

        restore_client_inner(&mut config, earlier.clone()).expect("restore");

        assert_eq!(config.approved_clients, [earlier]);
    }

    #[test]
    fn a_program_that_sent_no_name_cannot_be_restored() {
        let mut config = McpConfig::default();
        assert_eq!(
            restore_client_inner(&mut config, approved(" ", true, false)),
            Err(ApprovalError::NoName)
        );
    }

    #[test]
    fn a_waiting_file_caught_mid_write_is_not_an_empty_list() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        writ_storage::pending_clients::note_calls(dir.path(), &ClientId::named("Zed"), 1)
            .expect("note");
        let file = writ_storage::pending_clients::path(dir.path());
        let whole = std::fs::read(&file).expect("the file");
        std::fs::write(&file, &whole[..whole.len() - 3]).expect("cut it short");

        assert_eq!(waiting_settled_in(dir.path(), &[]), None);

        std::fs::write(&file, &whole).expect("the write completes");
        let names: Vec<String> = waiting_settled_in(dir.path(), &[])
            .expect("settled")
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, ["Zed"]);
    }

    #[test]
    fn the_settled_list_leaves_out_programs_already_decided_on() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        for name in ["Claude Code", "Zed"] {
            writ_storage::pending_clients::note_calls(dir.path(), &ClientId::named(name), 1)
                .expect("note");
        }

        let names: Vec<String> =
            waiting_settled_in(dir.path(), &[approved("Claude Code", false, false)])
                .expect("settled")
                .into_iter()
                .map(|entry| entry.name)
                .collect();
        assert_eq!(names, ["Zed"]);
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
    fn a_build_that_bundles_nothing_still_names_a_command() {
        // Under `cargo test` there is no bundled binary beside the test
        // executable, so this is the fallback path being selected.
        let command = resolved_command();
        assert!(command.command.ends_with("\" mcp"));
        assert!(!command.path.is_empty());
        assert!(!command.needs_move);
    }

    #[test]
    fn the_flags_cross_ipc_beside_the_command() {
        let json = serde_json::to_value(server_command_for(Path::new("/usr/local/bin/writ")))
            .expect("serialise");
        assert_eq!(json["needs_install"], false);
        assert_eq!(json["needs_move"], false);
        assert_eq!(json["command"], "\"/usr/local/bin/writ\" mcp");
    }
}
