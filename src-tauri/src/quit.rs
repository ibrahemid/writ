//! The shutdown handshake between the frontend and the exit path.
//!
//! Quitting inside the autosave debounce window used to lose whatever was
//! typed in it: nothing asks the webview to write before the process goes
//! away. The exit path now asks, and waits — but only as far as
//! [`writ_core::recovery::QUIT_FLUSH_TIMEOUT`], because a quit that hangs on
//! an unresponsive webview is a worse failure than a quit that loses the last
//! debounce window.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use writ_core::recovery::should_force_exit;

/// How often the wait re-reads the flag. Short enough that a confirmation
/// costs the user no perceptible part of the quit.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Records that the frontend has flushed every pending save.
///
/// Idempotent: the frontend may confirm twice, or after the wait has already
/// given up, and neither changes what the exit path does next.
pub fn confirm_flush(confirmed: &AtomicBool) {
    confirmed.store(true, Ordering::SeqCst);
}

/// Blocks until the frontend confirms it has flushed, or until the wait runs
/// out. Returns whether the confirmation arrived.
///
/// Polls rather than parking on a condition variable: a wakeup that lands
/// between the flag check and the wait is lost, and the one thing this must
/// never do is outlive the timeout.
pub fn wait_for_quit_flush(confirmed: &AtomicBool) -> bool {
    let started = Instant::now();
    loop {
        let flushed = confirmed.load(Ordering::SeqCst);
        if should_force_exit(started.elapsed(), flushed) {
            return flushed;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}
