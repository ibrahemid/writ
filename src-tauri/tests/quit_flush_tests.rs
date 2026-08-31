//! The shutdown handshake, driven through the flag rather than a real window.
//!
//! The end-to-end case — type, quit inside the debounce window, reopen the
//! note — is on the manual smoke list; what is worth pinning here is that the
//! wait ends on the confirmation, that it ends without one, and that a second
//! or late confirmation changes nothing.

use std::sync::Arc;
use std::time::{Duration, Instant};

use writ_core::recovery::QUIT_FLUSH_TIMEOUT;
use writ_tauri_lib::quit::{QuitDecision, QuitState};

#[test]
fn quit_waits_for_the_frontend_confirmation_before_snapshotting() {
    let quit = Arc::new(QuitState::new());
    let frontend = quit.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        frontend.confirm_flush();
    });

    let started = Instant::now();
    assert!(
        quit.wait_for_flush(),
        "the wait must report the confirmation it got"
    );
    let waited = started.elapsed();

    assert!(
        waited >= Duration::from_millis(80),
        "the shutdown path snapshotted before the frontend had flushed: {waited:?}"
    );
    assert!(
        waited < QUIT_FLUSH_TIMEOUT,
        "the wait ran on past the confirmation: {waited:?}"
    );
}

#[test]
fn quit_gives_up_on_a_webview_that_never_confirms() {
    let quit = QuitState::new();

    let started = Instant::now();
    assert!(
        !quit.wait_for_flush(),
        "an unanswered wait must report that it gave up"
    );
    let waited = started.elapsed();

    assert!(
        waited >= QUIT_FLUSH_TIMEOUT,
        "the wait ended before the timeout: {waited:?}"
    );
    assert!(
        waited < QUIT_FLUSH_TIMEOUT * 2,
        "the quit hung on an unresponsive webview: {waited:?}"
    );
}

#[test]
fn confirming_twice_or_after_the_wait_ended_changes_nothing() {
    let quit = QuitState::new();
    quit.confirm_flush();
    quit.confirm_flush();

    let started = Instant::now();
    assert!(quit.wait_for_flush());
    assert!(
        started.elapsed() < QUIT_FLUSH_TIMEOUT,
        "a second confirmation made the wait sit through the timeout"
    );

    quit.confirm_flush();
    assert!(quit.wait_for_flush());
}

// A quit request can arrive more than once: the user presses Cmd+Q again while
// the first one is still writing, and `AppHandle::exit` raises its own request
// on the way out. Only the first may start the work, only the last may exit.

#[test]
fn the_first_quit_request_starts_the_flush() {
    let quit = QuitState::new();
    assert_eq!(quit.begin(None), QuitDecision::StartFlush);
}

#[test]
fn a_second_quit_request_waits_for_the_one_already_running() {
    let quit = QuitState::new();
    quit.begin(None);
    assert_eq!(quit.begin(None), QuitDecision::Wait);
    assert_eq!(quit.begin(None), QuitDecision::Wait);
}

#[test]
fn a_quit_request_after_the_shutdown_work_proceeds() {
    let quit = QuitState::new();
    quit.begin(None);
    quit.finish();
    assert_eq!(quit.begin(None), QuitDecision::Proceed);
}

#[test]
fn the_requested_exit_code_survives_the_wait() {
    let quit = QuitState::new();
    quit.begin(Some(3));
    assert_eq!(quit.exit_code(), 3);
}

#[test]
fn a_request_without_a_code_exits_zero() {
    let quit = QuitState::new();
    quit.begin(None);
    assert_eq!(quit.exit_code(), 0);
}

#[test]
fn a_later_request_does_not_overwrite_the_code_the_first_one_carried() {
    let quit = QuitState::new();
    quit.begin(Some(7));
    quit.begin(Some(0));
    assert_eq!(quit.exit_code(), 7);
}

#[test]
fn the_final_shutdown_is_claimed_once() {
    let quit = QuitState::new();
    assert!(quit.claim_final_shutdown());
    assert!(!quit.claim_final_shutdown());
}

#[test]
fn a_termination_after_a_completed_shutdown_claims_nothing() {
    let quit = QuitState::new();
    quit.begin(None);
    quit.finish();
    assert!(!quit.claim_final_shutdown());
}
