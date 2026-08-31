//! The shutdown handshake, driven through the flag rather than a real window.
//!
//! The end-to-end case — type, quit inside the debounce window, reopen the
//! note — is on the manual smoke list; what is worth pinning here is that the
//! wait ends on the confirmation, that it ends without one, and that a second
//! or late confirmation changes nothing.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use writ_core::recovery::QUIT_FLUSH_TIMEOUT;
use writ_tauri_lib::quit::{confirm_flush, wait_for_quit_flush};

#[test]
fn quit_waits_for_the_frontend_confirmation_before_snapshotting() {
    let confirmed = Arc::new(AtomicBool::new(false));
    let frontend = confirmed.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        confirm_flush(&frontend);
    });

    let started = Instant::now();
    assert!(
        wait_for_quit_flush(&confirmed),
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
    let confirmed = AtomicBool::new(false);

    let started = Instant::now();
    assert!(
        !wait_for_quit_flush(&confirmed),
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
    let confirmed = AtomicBool::new(false);
    confirm_flush(&confirmed);
    confirm_flush(&confirmed);

    let started = Instant::now();
    assert!(wait_for_quit_flush(&confirmed));
    assert!(
        started.elapsed() < QUIT_FLUSH_TIMEOUT,
        "a second confirmation made the wait sit through the timeout"
    );

    confirm_flush(&confirmed);
    assert!(wait_for_quit_flush(&confirmed));
}
