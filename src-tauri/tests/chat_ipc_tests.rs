//! Coverage for the five chat-pane commands (ADR-031 section 4).
//!
//! Each is exercised through its Tauri-free inner function, against a real
//! notes folder and a real activity log, so the assertions cover what the pane
//! receives and what the folder holds afterwards rather than only the policy
//! underneath. The last test asserts every one of them is in the invoke
//! handler, since a command that is not registered cannot be called however
//! well it behaves.

use std::path::Path;

use writ_core::activity::{Actor, Decision};
use writ_core::chat::{ChatError, ChatTurn, Provider, Role};
use writ_core::config::{AiChatConfig, AiConfig};
use writ_tauri_lib::commands::ai::AiKeyState;
use writ_tauri_lib::commands::chat::{
    apply_proposal_inner, attached_sizes_in, discard_proposal_inner, endpoint_state_from,
    key_account, note_file_in, prepare_chat, read_attached_in, ChatState,
};

const LIB_RS: &str = include_str!("../src/lib.rs");

const COMMANDS: &[&str] = &[
    "commands::chat::chat_state",
    "commands::chat::chat_attached_sizes",
    "commands::chat::chat_send",
    "commands::chat::chat_cancel",
    "commands::chat::chat_apply_proposal",
    "commands::chat::chat_discard_proposal",
];

fn no_key() -> AiKeyState {
    AiKeyState {
        is_set: false,
        memory_only: false,
    }
}

fn config(base_url: &str, provider: &str) -> AiConfig {
    AiConfig {
        chat: AiChatConfig {
            enabled: true,
            provider: provider.to_string(),
            base_url: base_url.to_string(),
            model: "a-model".to_string(),
        },
        ..AiConfig::default()
    }
}

fn turn(content: &str) -> Vec<ChatTurn> {
    vec![ChatTurn {
        role: Role::User,
        content: content.to_string(),
    }]
}

/// A notes folder holding `Launch.md`, and a data folder for the log.
fn folders() -> (tempfile::TempDir, tempfile::TempDir) {
    let notes = tempfile::TempDir::new().expect("notes folder");
    let writ = tempfile::TempDir::new().expect("data folder");
    std::fs::write(notes.path().join("Launch.md"), "the first text\n").expect("write");
    (notes, writ)
}

fn root(dir: &tempfile::TempDir) -> std::path::PathBuf {
    std::fs::canonicalize(dir.path()).expect("canonical")
}

fn log_of(writ: &Path) -> Vec<writ_core::activity::ActivityRecord> {
    writ_storage::activity_log::read_recent(writ, 100)
}

// --- chat_state -------------------------------------------------------------

#[test]
fn chat_state_reports_where_a_hosted_endpoint_points_and_what_it_needs() {
    let cfg = config("https://api.example.com/v1", "openai_compatible");
    let state = endpoint_state_from(&cfg, no_key());
    assert!(state.enabled);
    assert_eq!(state.host.as_deref(), Some("api.example.com"));
    assert!(state.is_hosted);
    assert!(state.is_allowed);
    assert!(!state.is_consented, "a host nobody consented to");
    assert!(!state.key_state.is_set);
}

#[test]
fn chat_state_says_a_consented_host_is_consented() {
    let mut cfg = config("https://api.example.com/v1", "openai_compatible");
    cfg.consented_hosts = vec!["api.example.com".to_string()];
    assert!(endpoint_state_from(&cfg, no_key()).is_consented);
}

#[test]
fn chat_state_refuses_plaintext_to_a_remote_host() {
    let cfg = config("http://api.example.com/v1", "openai_compatible");
    let state = endpoint_state_from(&cfg, no_key());
    assert!(state.is_hosted);
    assert!(!state.is_allowed);
}

#[test]
fn chat_state_names_the_keychain_account_of_each_provider() {
    assert_eq!(
        key_account(&config("https://api.anthropic.com", "anthropic")),
        Some(Provider::Anthropic.key_account())
    );
    assert_eq!(Provider::Anthropic.key_account(), "chat:anthropic");
    // The rewrite path's account for that provider id would be the bare word,
    // and the two must never meet in the keychain.
    assert_ne!(
        key_account(&config("https://api.anthropic.com", "anthropic")),
        Some("anthropic")
    );
    assert_eq!(
        key_account(&config("http://localhost:11434/v1", "telepathy")),
        None
    );
}

// --- chat_attached_sizes ----------------------------------------------------

#[test]
fn chat_attached_sizes_answers_under_the_path_it_was_given() {
    // What the pane holds is the absolute source path, and it joins its own
    // list to this answer. An answer keyed only by the folder-relative name
    // would miss every row and leave the stale tab size in the dialog.
    let (notes, _writ) = folders();
    let root = root(&notes);
    std::fs::create_dir_all(root.join("Ideas")).expect("folder");
    std::fs::write(root.join("Ideas/Later.md"), "a longer second text\n").expect("write");
    let absolute = root.join("Ideas/Later.md").to_string_lossy().into_owned();

    let sizes = attached_sizes_in(&root, &[absolute.clone()]).expect("sizes");
    assert_eq!(sizes.len(), 1);
    assert_eq!(sizes[0].path, absolute, "the path asked about comes back");
    assert_eq!(sizes[0].key, "Ideas/Later.md");
    assert_eq!(sizes[0].bytes, "a longer second text\n".len() as u64);
}

#[test]
fn chat_attached_sizes_reads_the_bytes_the_file_holds_now() {
    let (notes, _writ) = folders();
    let root = root(&notes);
    std::fs::create_dir_all(root.join("Ideas")).expect("folder");
    std::fs::write(root.join("Ideas/Later.md"), "a longer second text\n").expect("write");

    let sizes = attached_sizes_in(
        &root,
        &["Launch.md".to_string(), "Ideas/Later.md".to_string()],
    )
    .expect("sizes");
    assert_eq!(sizes.len(), 2);
    assert_eq!(sizes[0].key, "Launch.md");
    assert_eq!(sizes[0].bytes, "the first text\n".len() as u64);
    assert_eq!(sizes[1].key, "Ideas/Later.md");
    assert_eq!(sizes[1].bytes, "a longer second text\n".len() as u64);

    // Rewritten by another program after the tab read it: the dialog states
    // what a send would carry, not what the tab remembers.
    std::fs::write(root.join("Launch.md"), "much more text than before\n").expect("rewrite");
    let sizes = attached_sizes_in(&root, &["Launch.md".to_string()]).expect("sizes");
    assert_eq!(sizes[0].bytes, "much more text than before\n".len() as u64);
}

#[test]
fn chat_attached_sizes_refuses_a_path_outside_the_notes_folder() {
    let (notes, _writ) = folders();
    let root = root(&notes);
    let outside = tempfile::TempDir::new().expect("elsewhere");
    std::fs::write(outside.path().join("Secret.md"), "not yours\n").expect("write");
    let path = std::fs::canonicalize(outside.path().join("Secret.md")).expect("canonical");

    let error =
        attached_sizes_in(&root, &[path.to_string_lossy().into_owned()]).expect_err("refused");
    assert!(error.contains("notes folder"), "got: {error}");
}

// --- chat_send --------------------------------------------------------------

#[test]
fn chat_send_carries_the_attached_notes_and_nothing_else() {
    let (notes, _writ) = folders();
    let root = root(&notes);
    std::fs::write(root.join("Other.md"), "the neighbouring text\n").expect("write");

    let attached = read_attached_in(&root, &["Launch.md".to_string()]).expect("attached");
    assert_eq!(attached.len(), 1, "one note was named, one was read");

    let prepared = prepare_chat(
        &config("http://localhost:11434/v1", "openai_compatible"),
        &turn("what does it argue"),
        attached,
        |_| None,
    )
    .expect("prepared");
    let body = serde_json::to_string(&prepared.body).expect("body");
    assert!(body.contains("the first text"));
    assert!(
        !body.contains("the neighbouring text"),
        "a note nobody attached reached the request"
    );
    assert_eq!(prepared.context[0].path, "Launch.md");
}

#[test]
fn chat_send_refuses_a_path_outside_the_notes_folder() {
    let (notes, _writ) = folders();
    let outside = tempfile::TempDir::new().expect("other folder");
    let stranger = outside.path().join("Secrets.md");
    std::fs::write(&stranger, "not yours\n").expect("write");

    let root = root(&notes);
    for path in [
        stranger.to_string_lossy().into_owned(),
        "../Secrets.md".to_string(),
    ] {
        let error = read_attached_in(&root, std::slice::from_ref(&path)).expect_err("refused");
        assert!(error.contains("notes folder"), "got: {error}");
    }
}

#[test]
fn chat_send_refuses_an_unconsented_hosted_host_before_the_body_is_built() {
    let error = prepare_chat(
        &config("https://api.example.com/v1", "openai_compatible"),
        &turn("hello"),
        Vec::new(),
        |_| panic!("the key was read for a host with no consent"),
    )
    .expect_err("refused");
    assert_eq!(
        error,
        ChatError::ConsentRequired {
            host: "api.example.com".to_string()
        }
    );
}

#[test]
fn chat_send_refuses_when_the_switch_is_off() {
    let mut cfg = config("http://localhost:11434/v1", "openai_compatible");
    cfg.chat.enabled = false;
    assert_eq!(
        prepare_chat(&cfg, &turn("hello"), Vec::new(), |_| None),
        Err(ChatError::Disabled)
    );
}

#[test]
fn chat_send_reads_one_note_once_however_often_it_was_named() {
    let (notes, _writ) = folders();
    let root = root(&notes);
    let attached = read_attached_in(
        &root,
        &[
            "Launch.md".to_string(),
            root.join("Launch.md").to_string_lossy().into_owned(),
        ],
    )
    .expect("attached");
    assert_eq!(attached.len(), 1);
}

// --- chat_cancel ------------------------------------------------------------

#[test]
fn chat_cancel_raises_the_flag_of_a_live_conversation_and_no_other() {
    let state = ChatState::default();
    let cancel = state.begin("c1");
    state.begin("c2");
    assert!(state.cancel("c1"));
    assert!(cancel.load(std::sync::atomic::Ordering::Relaxed));
    assert_eq!(state.live(), 2);

    state.finish("c1");
    assert!(
        !state.cancel("c1"),
        "a conversation that ended cancels nothing"
    );
    assert_eq!(state.live(), 1);
}

// --- chat_apply_proposal ----------------------------------------------------

#[test]
fn chat_apply_proposal_writes_the_note_and_records_it() {
    let (notes, writ) = folders();
    let root = root(&notes);
    let before = writ_core::hash::sha256_hex(b"the first text\n");

    let outcome = apply_proposal_inner(
        &root,
        writ.path(),
        "api.example.com",
        "Launch.md",
        "the second text\n",
        &before,
    )
    .expect("applied");

    assert_eq!(outcome.path, "Launch.md");
    assert_eq!(
        std::fs::read_to_string(root.join("Launch.md")).expect("read"),
        "the second text\n"
    );

    let log = log_of(writ.path());
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].action, "apply_proposal");
    assert_eq!(log[0].decision, Decision::Allow);
    assert_eq!(
        log[0].actor,
        Actor::Chat {
            host: "api.example.com".to_string()
        }
    );
    assert_eq!(log[0].bytes, Some("the second text\n".len() as u64));
}

#[test]
fn a_refused_note_in_a_subfolder_keeps_its_folder_relative_key() {
    let (notes, writ) = folders();
    let root = root(&notes);
    std::fs::create_dir_all(root.join("Ideas")).expect("folder");
    let before = writ_core::hash::sha256_hex(b"the first text\n");
    std::fs::write(root.join("Ideas/Launch.md"), "somebody else wrote this\n").expect("write");

    let error = apply_proposal_inner(
        &root,
        writ.path(),
        "api.example.com",
        "Ideas/Launch.md",
        "the model's text\n",
        &before,
    )
    .expect_err("refused");
    assert!(error.starts_with("Ideas/Launch.md "), "got: {error}");
}

#[test]
fn chat_apply_proposal_refuses_a_note_that_changed_and_leaves_a_copy() {
    let (notes, writ) = folders();
    let root = root(&notes);
    let before = writ_core::hash::sha256_hex(b"the first text\n");
    std::fs::write(root.join("Launch.md"), "somebody else wrote this\n").expect("write");

    let error = apply_proposal_inner(
        &root,
        writ.path(),
        "api.example.com",
        "Launch.md",
        "the model's text\n",
        &before,
    )
    .expect_err("refused");

    assert_eq!(
        std::fs::read_to_string(root.join("Launch.md")).expect("read"),
        "somebody else wrote this\n",
        "the refusal wrote nothing over the note"
    );

    let copies: Vec<String> = std::fs::read_dir(&root)
        .expect("read folder")
        .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
        .filter(|name| name != "Launch.md")
        .collect();
    assert_eq!(copies.len(), 1, "the refused text is on disk: {copies:?}");

    // The sentence the pane shows: the note by the key every other chat
    // surface names it by, and the copy the proposed text went to. No
    // absolute path, because the pane never shows one.
    assert!(error.contains("Launch.md"), "got: {error}");
    assert!(error.contains(&copies[0]), "got: {error}");
    assert!(
        !error.contains(root.to_string_lossy().as_ref()),
        "the refusal named the folder: {error}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join(&copies[0])).expect("read copy"),
        "the model's text\n"
    );

    let log = log_of(writ.path());
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].decision, Decision::Refuse);
    assert_eq!(log[0].bytes, None);
}

#[test]
fn chat_apply_proposal_refuses_a_path_outside_the_notes_folder() {
    let (notes, writ) = folders();
    let outside = tempfile::TempDir::new().expect("other folder");
    let stranger = outside.path().join("Secrets.md");
    std::fs::write(&stranger, "not yours\n").expect("write");
    let before = writ_core::hash::sha256_hex(b"not yours\n");

    let error = apply_proposal_inner(
        &root(&notes),
        writ.path(),
        "api.example.com",
        &stranger.to_string_lossy(),
        "owned\n",
        &before,
    )
    .expect_err("refused");
    assert!(error.contains("notes folder"), "got: {error}");
    assert_eq!(
        std::fs::read_to_string(&stranger).expect("read"),
        "not yours\n"
    );
    assert!(log_of(writ.path()).is_empty());
}

#[test]
fn chat_apply_proposal_refuses_a_hash_it_cannot_read() {
    let (notes, writ) = folders();
    let root = root(&notes);
    let error = apply_proposal_inner(
        &root,
        writ.path(),
        "api.example.com",
        "Launch.md",
        "the model's text\n",
        "not-a-digest",
    )
    .expect_err("refused");
    assert!(error.contains("Launch.md"), "got: {error}");
    assert_eq!(
        std::fs::read_to_string(root.join("Launch.md")).expect("read"),
        "the first text\n"
    );
}

// --- chat_discard_proposal --------------------------------------------------

#[test]
fn chat_discard_proposal_records_the_offer_and_touches_no_file() {
    let (notes, writ) = folders();
    let root = root(&notes);
    discard_proposal_inner(&root, writ.path(), "api.example.com", "Launch.md");

    assert_eq!(
        std::fs::read_to_string(root.join("Launch.md")).expect("read"),
        "the first text\n"
    );
    let log = log_of(writ.path());
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].action, "discard_proposal");
    assert_eq!(log[0].decision, Decision::Refuse);
    assert_eq!(log[0].path.as_deref(), Some(Path::new("Launch.md")));
    assert_eq!(log[0].bytes, None, "a discard wrote no bytes");
}

// --- Registration -----------------------------------------------------------

#[test]
fn every_chat_command_is_in_the_invoke_handler() {
    for command in COMMANDS {
        assert!(
            LIB_RS.contains(command),
            "{command} is not registered in the invoke handler"
        );
    }
}

#[test]
fn a_note_the_folder_does_not_hold_is_not_a_note() {
    let (notes, _writ) = folders();
    let root = root(&notes);
    let error = note_file_in(&root, "Nowhere.md").expect_err("refused");
    assert!(error.contains("Nowhere.md"), "got: {error}");
}
