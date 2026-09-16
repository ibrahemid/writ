//! The registry behind one live reply per conversation (ADR-040 section 7).
//!
//! A send is `(conversation_id, request_id)` from here on, so these cover what
//! the pair buys: a second send cannot slip past the refusal, a stop names the
//! request it means, a task that fell over releases its conversation, and a
//! finish that arrives late leaves a newer request alone.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Barrier};

use tempfile::TempDir;
use writ_storage::chat_store::ChatStore;
use writ_tauri_lib::commands::chat::{
    begin_request, chat_delete_inner, ChatState, LiveGuard, REPLY_IN_FLIGHT,
};

const C1: &str = "0b7d6b7a-1111-4b6a-9d5e-000000000001";
const C2: &str = "0b7d6b7a-2222-4b6a-9d5e-000000000002";

#[test]
fn a_second_send_on_a_live_conversation_is_refused_atomically() {
    // The old shape read `is_live` and inserted under two separate locks, with
    // a file read, a keychain read and a save in between, so two sends could
    // both find the conversation free and the second would take the first's
    // cancel flag.
    for _ in 0..64 {
        let state = ChatState::default();
        let gate = Arc::new(Barrier::new(2));
        let racers: Vec<_> = ["r-a", "r-b"]
            .into_iter()
            .map(|request_id| {
                let state = state.clone();
                let gate = gate.clone();
                std::thread::spawn(move || {
                    gate.wait();
                    begin_request(&state, C1, request_id).map(|(_, guard)| guard)
                })
            })
            .collect();

        let outcomes: Vec<_> = racers
            .into_iter()
            .map(|racer| racer.join().expect("the sending thread ended"))
            .collect();

        let accepted = outcomes.iter().filter(|outcome| outcome.is_ok()).count();
        assert_eq!(accepted, 1, "both sends were accepted for one conversation");
        let refusal = outcomes
            .iter()
            .find_map(|outcome| outcome.as_ref().err())
            .expect("one send was refused");
        assert_eq!(refusal, REPLY_IN_FLIGHT);
        assert_eq!(state.live(), 1, "the refused send left an entry behind");
    }
}

#[test]
fn stop_with_a_stale_request_id_does_not_cancel_the_newer_request() {
    let state = ChatState::default();
    let (cancel, _guard) = begin_request(&state, C1, "r-2").expect("the send was accepted");

    assert!(
        !state.cancel(C1, Some("r-1")),
        "a stop naming a request that had already ended cancelled the live one"
    );
    assert!(!cancel.load(Ordering::Relaxed));

    assert!(state.cancel(C1, Some("r-2")));
    assert!(cancel.load(Ordering::Relaxed));
}

#[test]
fn stop_without_an_id_cancels_whatever_is_live() {
    // Shutdown has no request id to name: it stops whatever each conversation
    // is doing.
    let state = ChatState::default();
    let (first, _first_guard) = begin_request(&state, C1, "r-1").expect("accepted");
    let (second, _second_guard) = begin_request(&state, C2, "r-9").expect("accepted");

    assert!(state.cancel(C1, None));
    assert!(first.load(Ordering::Relaxed));
    assert!(
        !second.load(Ordering::Relaxed),
        "a stop reached a conversation nobody named"
    );
    assert!(
        !state.cancel("0b7d6b7a-3333-4b6a-9d5e-000000000003", None),
        "a stop found something live under an id nothing is running for"
    );
}

#[test]
fn a_panicking_task_releases_the_conversation() {
    // `finish` used to be the last statement of the spawned task, so a panic
    // anywhere in it marked the conversation live for the life of the process
    // and no send or stop could clear it.
    let state = ChatState::default();
    let (_cancel, guard) = begin_request(&state, C1, "r-1").expect("accepted");

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let fell_over = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _held = guard;
        panic!("the request task fell over");
    }));
    std::panic::set_hook(hook);

    assert!(fell_over.is_err());
    assert!(
        !state.is_live(C1),
        "the conversation stayed live after a panic"
    );
    assert!(begin_request(&state, C1, "r-2").is_ok());
}

#[test]
fn finish_of_an_older_request_leaves_the_newer_entry() {
    let state = ChatState::default();
    let stale = LiveGuard::new(state.clone(), C1.to_string(), "r-1".to_string());
    let (cancel, _guard) = begin_request(&state, C1, "r-2").expect("accepted");

    drop(stale);

    assert!(state.is_live(C1), "a finished request removed a newer one");
    assert!(state.cancel(C1, Some("r-2")));
    assert!(cancel.load(Ordering::Relaxed));
}

#[test]
fn delete_cancels_the_live_request() {
    // Without the cancel the task streams on into a file that is gone: every
    // chunk reaches a pane with nowhere to put it and the save at the end
    // fails with a warning nobody sees.
    let dir = TempDir::new().expect("temp writ dir");
    let store = ChatStore::new(dir.path());
    let conversation = store
        .create("openai", "gpt-4o-mini")
        .expect("a conversation");
    let state = ChatState::default();
    let (cancel, _guard) = begin_request(&state, &conversation.id, "r-1").expect("accepted");

    chat_delete_inner(&store, &state, &conversation.id).expect("deleted");

    assert!(
        cancel.load(Ordering::Relaxed),
        "the reply kept streaming into a conversation that no longer exists"
    );
    assert!(store.load(&conversation.id).is_err());
}

#[test]
fn a_refused_send_leaves_no_live_entry() {
    // Everything a send refuses after the registry entry is taken — a missing
    // conversation, a full one, a connection with no key, a client that would
    // not build — returns through the guard, which is the only thing that
    // removes the entry.
    let state = ChatState::default();
    let refused: Result<(), String> = (|| {
        let (_cancel, _guard) = begin_request(&state, C1, "r-1")?;
        Err("this chat is full".to_string())
    })();

    assert_eq!(refused, Err("this chat is full".to_string()));
    assert!(!state.is_live(C1));
    assert_eq!(state.live(), 0);
    assert!(
        begin_request(&state, C1, "r-2").is_ok(),
        "a refused send left the conversation unusable"
    );
}
