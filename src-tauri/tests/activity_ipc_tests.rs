//! Coverage for the activity and connected-program commands (ADR-031).
//!
//! Each is exercised through its Tauri-free inner function, against a real log
//! folder and a real `[mcp]` section, so the assertions cover what the panel
//! and the settings section receive rather than only the policy underneath.
//! The last test asserts every one of them is in the invoke handler, since a
//! command that is not registered cannot be called however well it behaves.

use writ_core::activity::{ActivityRecord, Actor, ClientId, Decision, PendingClient};
use writ_core::config::mcp::{ClientApproval, McpConfig};
use writ_core::config::WritConfig;
use writ_mcp::consent::{ConfigGate, ConsentGate, Verdict};
use writ_storage::config_store::ConfigStore;
use writ_tauri_lib::commands::activity::{
    activity_clear_inner, activity_recent_inner, announce_waiting_inner, deny_client_inner,
    deny_unnamed_inner, dialog_name, forget_client_inner, is_new_write_grant, mcp_clients_inner,
    mcp_tools, restore_client_inner, return_to_waiting_inner, server_command_for,
    set_client_permission_inner, waiting_in, ApprovalError, MAX_ACTIVITY_LIMIT,
};
use writ_tauri_lib::events::emitter::WritFrontendEvent;
use writ_tauri_lib::state::AppState;

const LIB_RS: &str = include_str!("../src/lib.rs");
const ACTIVITY_RS: &str = include_str!("../src/commands/activity.rs");

const COMMANDS: &[&str] = &[
    "commands::activity::activity_recent",
    "commands::activity::activity_clear",
    "commands::activity::mcp_clients",
    "commands::activity::mcp_set_client_permission",
    "commands::activity::mcp_forget_client",
    "commands::activity::mcp_deny_client",
    "commands::activity::mcp_return_to_waiting",
    "commands::activity::mcp_restore_client",
    "commands::activity::mcp_server_command",
    "commands::activity::mcp_tools",
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
        context: false,
        review: false,
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
    assert!(
        body_of("write_approvals").contains("tell_settings_moved(app)"),
        "an approval written from the app must announce the settings change"
    );
    assert!(
        body_of("mcp_return_to_waiting").contains("tell_settings_moved(&app)"),
        "undoing a denial writes the settings too"
    );
    assert!(body_of("tell_settings_moved").contains("WritFrontendEvent::ConfigChanged"));
}

fn body_of(name: &str) -> &'static str {
    let start = ACTIVITY_RS
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("{name} is in commands/activity.rs"));
    let rest = &ACTIVITY_RS[start..];
    let end = rest.find("\n}\n").expect("the function ends");
    &rest[..end]
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
        context: false,
        review: false,
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
        context: false,
        review: false,
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

/// The settings row tells the user what an approved program can do. A
/// hand-written list there would go stale the first time a tool was added, and
/// the user would be reading a promise the server no longer keeps. Both sides
/// read `writ_core::tools`; this is the assertion that they agree.
#[test]
fn the_settings_row_lists_the_tools_the_served_process_registers() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let host = writ_mcp::tools::ToolHost::open(
        dir.path(),
        &dir.path().join("writ.db"),
        dir.path(),
        Box::new(writ_mcp::tools::FixedAppFolder(dir.path().to_path_buf())),
        Box::new(writ_mcp::consent::DenyAll),
    )
    .expect("host");
    let mut registered = writ_mcp::server::WritServer::new(host).tool_names();
    registered.sort();

    let tools = mcp_tools();
    let mut shown: Vec<String> = tools
        .read
        .into_iter()
        .chain(tools.write)
        .chain(tools.context)
        .collect();
    shown.sort();

    assert_eq!(shown, registered);
}

#[test]
fn the_write_half_of_the_settings_row_is_the_six_write_tools() {
    let tools = mcp_tools();

    assert_eq!(
        tools.write,
        vec![
            "write_note",
            "create_note",
            "rename_note",
            "append_note",
            "replace_in_note",
            "today"
        ]
    );
    for absent in ["delete_note", "trash_note", "move_note"] {
        assert!(!tools.read.contains(&absent.to_string()), "{absent}");
        assert!(!tools.write.contains(&absent.to_string()), "{absent}");
    }
}

fn write_settings(dir: &std::path::Path, mcp: McpConfig) {
    let store = ConfigStore::new(dir.join("config.toml"));
    let config = WritConfig {
        mcp,
        ..WritConfig::default()
    };
    let text = store.serialize(&config).expect("serialise");
    store.write_serialized(&text).expect("write");
}

fn read_settings(dir: &std::path::Path) -> McpConfig {
    ConfigStore::new(dir.join("config.toml"))
        .read()
        .expect("read")
        .mcp
}

// `AppState::initialize` reads `WRIT_DATA_DIR` and `WRIT_NOTES_DIR`; the tests
// in this file that set them take turns.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Launched {
    state: AppState,
    _data: tempfile::TempDir,
    _notes: tempfile::TempDir,
}

fn launch() -> Launched {
    let data = tempfile::TempDir::new().expect("data dir");
    let notes = tempfile::TempDir::new().expect("notes dir");
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("WRIT_DATA_DIR", data.path());
    std::env::set_var("WRIT_NOTES_DIR", notes.path());
    let state = AppState::initialize().expect("app state");
    std::env::remove_var("WRIT_DATA_DIR");
    std::env::remove_var("WRIT_NOTES_DIR");
    Launched {
        state,
        _data: data,
        _notes: notes,
    }
}

fn decide_in(state: &AppState, change: impl FnOnce(&mut McpConfig)) {
    let mut config = state.config.lock().expect("config");
    change(&mut config.mcp);
    config.mcp.enabled = true;
    write_settings(&state.writ_dir, config.mcp.clone());
}

fn names(waiting: &[PendingClient]) -> Vec<&str> {
    waiting.iter().map(|entry| entry.name.as_str()).collect()
}

#[test]
fn a_denial_persists_and_can_be_undone() {
    let app = launch();
    let dir = app.state.writ_dir.clone();
    let program = ClientId::named("Claude Code");
    decide_in(&app.state, |mcp| {
        deny_client_inner(mcp, "Claude Code").expect("deny");
    });

    let stored = read_settings(&dir);
    let denial = stored
        .approval_for("Claude Code")
        .expect("the denial is stored");
    assert!(!denial.read && !denial.write);
    assert_eq!(
        mcp_clients_inner(&stored).len(),
        1,
        "listed with the decided programs"
    );

    let gate = ConfigGate::new(&dir);
    assert_eq!(gate.decide(&program, "read_note"), Verdict::NotGranted);
    assert!(
        waiting_in(&dir, &mcp_clients_inner(&stored)).is_empty(),
        "a program turned down is not waiting"
    );

    let clients =
        return_to_waiting_inner(&app.state, "Claude Code", || {}).expect("a denial to undo");

    assert!(clients.approved.is_empty());
    assert_eq!(names(&clients.waiting), ["Claude Code"]);
    assert!(read_settings(&dir).approval_for("Claude Code").is_none());
    assert_eq!(gate.decide(&program, "read_note"), Verdict::Waiting);
}

#[test]
fn undoing_a_denial_writes_the_settings_then_says_so_then_lists_the_program() {
    let app = launch();
    let dir = app.state.writ_dir.clone();
    decide_in(&app.state, |mcp| {
        deny_client_inner(mcp, "Claude Code").expect("deny");
    });

    let mut told = false;
    return_to_waiting_inner(&app.state, "Claude Code", || {
        told = true;
        assert!(
            read_settings(&dir).approval_for("Claude Code").is_none(),
            "the frontend was told before the settings were written"
        );
        assert!(
            writ_storage::pending_clients::read(&dir).is_empty(),
            "the program was listed before the frontend was told"
        );
    })
    .expect("undone");

    assert!(told);
    assert_eq!(
        names(&writ_storage::pending_clients::read(&dir)),
        ["Claude Code"]
    );
}

#[test]
fn an_undo_whose_settings_cannot_be_written_keeps_the_denial() {
    let app = launch();
    let dir = app.state.writ_dir.clone();
    decide_in(&app.state, |mcp| {
        deny_client_inner(mcp, "Claude Code").expect("deny");
    });
    let settings = dir.join("config.toml");
    std::fs::remove_file(&settings).expect("the settings file");
    std::fs::create_dir(&settings).expect("a folder where the settings file was");

    let mut told = false;
    let answer = return_to_waiting_inner(&app.state, "Claude Code", || told = true);

    assert!(answer.is_err());
    assert!(!told);
    let config = app.state.config.lock().expect("config");
    assert!(config
        .mcp
        .approval_for("Claude Code")
        .is_some_and(|approval| approval.grants_nothing()));
    assert!(writ_storage::pending_clients::read(&dir).is_empty());
}

#[test]
fn undoing_leaves_a_program_that_was_since_allowed_alone() {
    let app = launch();
    let dir = app.state.writ_dir.clone();
    decide_in(&app.state, |mcp| {
        set_client_permission_inner(mcp, "Claude Code", true, false).expect("allow");
    });

    let mut told = false;
    let clients =
        return_to_waiting_inner(&app.state, "Claude Code", || told = true).expect("answered");

    assert!(!told);
    assert_eq!(clients.approved.len(), 1);
    assert!(clients.waiting.is_empty());
    assert!(read_settings(&dir)
        .approval_for("Claude Code")
        .is_some_and(|approval| approval.read));
    assert!(!writ_storage::pending_clients::path(&dir).exists());
}

#[test]
fn not_allowing_a_program_that_sent_no_name_takes_its_row_off_the_list() {
    let app = launch();
    for name in ["", "Zed"] {
        writ_storage::pending_clients::note_calls(&app.state.writ_dir, &ClientId::named(name), 1)
            .expect("note");
    }

    let clients = deny_unnamed_inner(&app.state, "").expect("taken off");

    assert_eq!(names(&clients.waiting), ["Zed"]);
    assert!(clients.approved.is_empty());
    assert!(app
        .state
        .config
        .lock()
        .expect("config")
        .mcp
        .approved_clients
        .is_empty());
}

#[test]
fn a_forgotten_program_can_be_put_back() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let before = approved("Claude Code", true, true);
    let mut mcp = McpConfig {
        enabled: true,
        approved_clients: vec![before.clone()],
    };
    forget_client_inner(&mut mcp, "Claude Code");
    write_settings(dir.path(), mcp.clone());
    let gate = ConfigGate::new(dir.path());
    assert_eq!(
        gate.decide(&ClientId::named("Claude Code"), "write_note"),
        Verdict::Waiting
    );

    restore_client_inner(&mut mcp, before.clone()).expect("restore");
    write_settings(dir.path(), mcp);

    let stored = read_settings(dir.path());
    let restored = stored.approval_for("Claude Code").expect("back");
    assert!(restored.read && restored.write);
    assert_eq!(
        restored.first_seen.timestamp(),
        before.first_seen.timestamp()
    );
    assert_eq!(
        gate.decide(&ClientId::named("Claude Code"), "write_note"),
        Verdict::Allow
    );
}

#[test]
fn the_list_the_watcher_sends_is_the_one_on_disk_minus_the_decided() {
    let app = launch();
    decide_in(&app.state, |mcp| {
        deny_client_inner(mcp, "Zed").expect("deny");
    });
    for name in ["Claude Code", "Zed"] {
        writ_storage::pending_clients::note_calls(&app.state.writ_dir, &ClientId::named(name), 1)
            .expect("note");
    }

    let Some(WritFrontendEvent::McpPending { waiting }) = announce_waiting_inner(&app.state) else {
        panic!("a settled file is announced");
    };
    assert_eq!(names(&waiting), ["Claude Code"]);
}

#[test]
fn a_waiting_file_caught_mid_write_is_not_announced() {
    let app = launch();
    writ_storage::pending_clients::note_calls(&app.state.writ_dir, &ClientId::named("Zed"), 1)
        .expect("note");
    let file = writ_storage::pending_clients::path(&app.state.writ_dir);
    let whole = std::fs::read(&file).expect("the file");
    std::fs::write(&file, &whole[..whole.len() - 3]).expect("cut it short");

    assert_eq!(announce_waiting_inner(&app.state), None);
}

#[test]
fn turning_a_program_down_has_one_command() {
    assert!(!LIB_RS.contains("mcp_refuse_waiting_client"));
    assert!(!ACTIVITY_RS.contains("fn mcp_refuse_waiting_client"));
}

#[test]
fn every_app_write_to_the_waiting_file_is_stamped_first() {
    let source = ACTIVITY_RS
        .split_once("#[cfg(test)]")
        .expect("the module has tests")
        .0;
    let mut writes = 0;
    for call in ["pending_clients::forget(", "pending_clients::put_back("] {
        for (index, _) in source.match_indices(call) {
            writes += 1;
            let rest = &source[index..];
            let end = rest.find(';').expect("a statement");
            assert!(
                rest[..end].contains("stamp_waiting_write"),
                "{call} is written without a stamp: {}",
                &rest[..end]
            );
        }
    }
    assert!(
        writes >= 3,
        "deny, undo and every decision write the waiting file"
    );
}

/// Writing is the grant a script in the webview could hand a program, so it is
/// the one that waits on a native dialog; reading, a denial, a revocation and
/// a grant already made ask nothing.
#[test]
fn only_a_grant_of_writing_a_program_lacks_is_confirmed() {
    let mut config = McpConfig::default();
    assert!(is_new_write_grant(&config, "claude", true));
    assert!(!is_new_write_grant(&config, "claude", false));

    set_client_permission_inner(&mut config, "claude", true, false).expect("read");
    assert!(is_new_write_grant(&config, "claude", true));

    set_client_permission_inner(&mut config, "claude", true, true).expect("write");
    assert!(!is_new_write_grant(&config, "claude", true));
    assert!(!is_new_write_grant(&config, "claude", false));
    assert!(is_new_write_grant(&config, "claude-2", true));
}

#[test]
fn the_confirmation_shows_a_programs_name_on_one_short_line() {
    assert_eq!(dialog_name("claude-code"), "claude-code");
    assert_eq!(dialog_name("evil\nAllow it\r\tnow"), "evil Allow it  now");
    let long = "x".repeat(200);
    let shown = dialog_name(&long);
    assert_eq!(shown.chars().count(), 61);
    assert!(shown.ends_with('…'));
}

/// Both commands that can hand a program writing ask first. The dialog itself
/// needs a person at the app; this pins that neither command skips it.
#[test]
fn both_commands_that_can_grant_writing_ask_first() {
    for command in [
        "pub async fn mcp_set_client_permission(",
        "pub async fn mcp_restore_client(",
    ] {
        let start = ACTIVITY_RS
            .find(command)
            .unwrap_or_else(|| panic!("{command} is not an async command in activity.rs"));
        let body = &ACTIVITY_RS[start..];
        let body = &body[..body.find("\n}\n").expect("the command ends")];
        let asks = body
            .find("write_grant_allowed(")
            .expect("asks before writing");
        let writes = body.find("write_approvals(").expect("writes the approval");
        assert!(
            asks < writes,
            "{command} writes the approval before it asks"
        );
    }
}
