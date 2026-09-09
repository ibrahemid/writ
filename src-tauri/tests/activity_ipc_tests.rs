//! Coverage for the six activity and connected-program commands (ADR-031).
//!
//! Each is exercised through its Tauri-free inner function, against a real log
//! folder and a real `[mcp]` section, so the assertions cover what the panel
//! and the settings section receive rather than only the policy underneath.
//! The last test asserts every one of them is in the invoke handler, since a
//! command that is not registered cannot be called however well it behaves.

use writ_core::activity::{ActivityRecord, Actor, ClientId, Decision};
use writ_core::config::mcp::{ClientApproval, McpConfig};
use writ_tauri_lib::commands::activity::{
    activity_clear_inner, activity_recent_inner, forget_client_inner, mcp_clients_inner,
    server_command_for, set_client_permission_inner, waiting_in, ApprovalError, MAX_ACTIVITY_LIMIT,
};

const LIB_RS: &str = include_str!("../src/lib.rs");
const ACTIVITY_RS: &str = include_str!("../src/commands/activity.rs");

const COMMANDS: &[&str] = &[
    "commands::activity::activity_recent",
    "commands::activity::activity_clear",
    "commands::activity::mcp_clients",
    "commands::activity::mcp_set_client_permission",
    "commands::activity::mcp_forget_client",
    "commands::activity::mcp_server_command",
];

fn record(action: &str, decision: Decision) -> ActivityRecord {
    ActivityRecord::now(
        Actor::Client {
            name: "Claude Code".to_string(),
            version: Some("1.2.3".to_string()),
        },
        action,
        decision,
    )
    .with_path("Ideas/Tessera.md")
    .with_bytes(412)
}

fn seeded(dir: &std::path::Path, count: usize) {
    for index in 0..count {
        writ_storage::activity_log::append(dir, &record(&format!("call_{index}"), Decision::Allow))
            .expect("append");
    }
}

fn approved(name: &str, read: bool, write: bool) -> ClientApproval {
    ClientApproval {
        name: name.to_string(),
        first_seen: chrono::Utc::now(),
        read,
        write,
    }
}

#[test]
fn activity_recent_answers_newest_first() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    seeded(dir.path(), 4);

    let recent = activity_recent_inner(dir.path(), 2);
    let actions: Vec<&str> = recent.iter().map(|r| r.action.as_str()).collect();
    assert_eq!(actions, ["call_3", "call_2"]);
}

#[test]
fn activity_recent_holds_the_panel_to_the_ceiling() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    seeded(dir.path(), 3);

    assert_eq!(activity_recent_inner(dir.path(), usize::MAX).len(), 3);

    // The ceiling is what a limit larger than it is held to, so a panel asking
    // for everything cannot ask for more than the log will hand back.
    seeded(dir.path(), MAX_ACTIVITY_LIMIT + 5);
    assert_eq!(
        activity_recent_inner(dir.path(), usize::MAX).len(),
        MAX_ACTIVITY_LIMIT
    );
}

#[test]
fn activity_recent_over_a_folder_with_no_log_is_empty() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    assert!(activity_recent_inner(dir.path(), 50).is_empty());
}

#[test]
fn activity_recent_carries_the_decision_and_the_path() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    writ_storage::activity_log::append(dir.path(), &record("write_note", Decision::Pending))
        .expect("append");

    let recent = activity_recent_inner(dir.path(), 10);
    assert_eq!(recent[0].decision, Decision::Pending);
    assert_eq!(
        recent[0].path.as_deref(),
        Some(std::path::Path::new("Ideas/Tessera.md"))
    );
    assert_eq!(recent[0].bytes, Some(412));
}

#[test]
fn activity_clear_empties_the_list() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    seeded(dir.path(), 3);

    activity_clear_inner(dir.path()).expect("clear");

    assert!(activity_recent_inner(dir.path(), 50).is_empty());
}

#[test]
fn activity_clear_over_a_folder_with_no_log_is_not_an_error() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    activity_clear_inner(dir.path()).expect("clear");
}

#[test]
fn mcp_clients_answers_the_approvals_by_name() {
    let config = McpConfig {
        enabled: true,
        approved_clients: vec![
            approved("Zed", true, false),
            approved("Claude Code", true, true),
        ],
    };

    let listed = mcp_clients_inner(&config);
    assert_eq!(listed[0].name, "Claude Code");
    assert!(listed[0].write);
    assert_eq!(listed[1].name, "Zed");
    assert!(!listed[1].write);
}

#[test]
fn mcp_clients_over_a_fresh_configuration_is_empty() {
    assert!(mcp_clients_inner(&McpConfig::default()).is_empty());
}

#[test]
fn mcp_set_client_permission_grants_one_direction_at_a_time() {
    let mut config = McpConfig::default();

    set_client_permission_inner(&mut config, "Claude Code", true, false).expect("set");
    let approval = config.approval_for("Claude Code").expect("the client");
    assert!(approval.read);
    assert!(!approval.write);

    set_client_permission_inner(&mut config, "Claude Code", true, true).expect("set");
    assert!(config.approval_for("Claude Code").expect("client").write);
}

#[test]
fn mcp_set_client_permission_stores_the_name_the_client_sent() {
    let mut config = McpConfig::default();
    set_client_permission_inner(&mut config, " Claude Code ", true, false).expect("set");

    // Exactly as sent, because that is the string the gate matches on.
    assert!(config.approval_for(" Claude Code ").is_some());
    assert!(config.approval_for("Claude Code").is_none());
}

#[test]
fn mcp_set_client_permission_needs_a_name() {
    let mut config = McpConfig::default();
    assert_eq!(
        set_client_permission_inner(&mut config, "  ", true, true),
        Err(ApprovalError::NoName)
    );
}

#[test]
fn mcp_forget_client_returns_the_client_to_pending() {
    let mut config = McpConfig {
        enabled: true,
        approved_clients: vec![approved("Claude Code", true, true)],
    };

    forget_client_inner(&mut config, "Claude Code");

    assert!(config.approval_for("Claude Code").is_none());
    assert!(mcp_clients_inner(&config).is_empty());
}

#[test]
fn mcp_server_command_names_an_absolute_binary_and_the_verb() {
    let command = server_command_for(std::path::Path::new(
        "/Applications/Writ.app/Contents/MacOS/writ",
    ));

    assert!(command.command.ends_with(" mcp"));
    assert!(command.command.contains("/Applications/Writ.app"));
    assert_eq!(command.path, "/Applications/Writ.app/Contents/MacOS/writ");
}

#[test]
fn every_command_is_in_the_invoke_handler() {
    for command in COMMANDS {
        assert!(
            LIB_RS.contains(command),
            "{command} is not in the invoke handler, so the editor cannot call it"
        );
    }
}

/// `persist_config` puts the write in the watcher's ignore set, so an approval
/// saved from the app is never reported back as an external edit. Without an
/// event of its own the frontend keeps the settings it loaded, and the next
/// settings edit sends that whole stale copy back and drops the approval.
#[test]
fn saving_an_approval_tells_the_frontend_the_settings_moved() {
    let body = ACTIVITY_RS
        .split_once("fn write_approvals(")
        .expect("write_approvals is where the approvals are persisted")
        .1;

    assert!(
        body.contains("WritFrontendEvent::ConfigChanged"),
        "an approval written from the app must announce the settings change"
    );
}

/// A program that only made calls the gate refused is still one the user has to
/// decide on, so it comes back from `mcp_clients` even though the settings hold
/// no approval for it.
#[test]
fn a_waiting_program_is_listed_beside_the_approved_ones() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    writ_storage::pending_clients::note_calls(dir.path(), &ClientId::named("Zed"), 3)
        .expect("note");

    let approved = vec![ClientApproval {
        name: "Claude Code".to_string(),
        first_seen: chrono::Utc::now(),
        read: true,
        write: false,
    }];

    let waiting = waiting_in(dir.path(), &approved);
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].name, "Zed");
    assert_eq!(waiting[0].calls, 3);
}

/// The approval is what counts: an entry the server left behind is not a second
/// decision to make.
#[test]
fn a_program_already_decided_on_is_not_also_waiting() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    writ_storage::pending_clients::note_calls(dir.path(), &ClientId::named("Claude Code"), 1)
        .expect("note");

    let approved = vec![ClientApproval {
        name: "Claude Code".to_string(),
        first_seen: chrono::Utc::now(),
        read: false,
        write: false,
    }];

    assert!(waiting_in(dir.path(), &approved).is_empty());
}

#[test]
fn a_folder_with_no_waiting_file_lists_nobody() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    assert!(waiting_in(dir.path(), &[]).is_empty());
}

/// Both write commands drop the entry, so the panel stops offering a decision
/// that has been made.
#[test]
fn deciding_on_a_program_is_wired_to_clear_its_waiting_entry() {
    let body = ACTIVITY_RS
        .split_once("fn write_approvals(")
        .expect("write_approvals is where both write commands land")
        .1;

    assert!(
        body.contains("pending_clients::forget"),
        "a decided program must be taken off the waiting list"
    );
}
