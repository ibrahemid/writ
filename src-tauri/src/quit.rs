//! The shutdown handshake between the frontend and the exit path.
//!
//! Quitting inside the autosave debounce window used to lose whatever was
//! typed in it: nothing asks the webview to write before the process goes
//! away. The exit path now asks, and waits — but only as far as
//! [`writ_core::recovery::QUIT_FLUSH_TIMEOUT`], because a quit that hangs on
//! an unresponsive webview is a worse failure than a quit that loses the last
//! debounce window.
//!
//! A quit request is not a single event. The user can press Cmd+Q again while
//! the first request is still writing, and `AppHandle::exit` raises a request
//! of its own on the way out. [`QuitState`] is the phase that keeps those from
//! racing: the first request does the work, the ones that follow hold the exit
//! until it is finished, and only then does the exit go through.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use writ_core::recovery::should_force_exit;

/// How often the wait re-reads the flag. Short enough that a confirmation
/// costs the user no perceptible part of the quit.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

const IDLE: u8 = 0;
const FLUSHING: u8 = 1;
const COMPLETE: u8 = 2;

/// What the exit path should do with the quit request it just received.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuitDecision {
    /// Nothing has been written yet. Hold the exit, ask the frontend to flush,
    /// and run the shutdown work.
    StartFlush,
    /// A request arrived while the first one is still working. Hold the exit
    /// too: the request in flight will exit once its writes are on disk.
    Wait,
    /// The shutdown work is done. Let the exit through.
    Proceed,
}

/// The shutdown phase and the frontend's answer to the flush request.
pub struct QuitState {
    phase: AtomicU8,
    flush_confirmed: AtomicBool,
    exit_code: AtomicI32,
}

impl Default for QuitState {
    fn default() -> Self {
        Self::new()
    }
}

impl QuitState {
    pub fn new() -> Self {
        Self {
            phase: AtomicU8::new(IDLE),
            flush_confirmed: AtomicBool::new(false),
            exit_code: AtomicI32::new(0),
        }
    }

    /// Takes up a quit request, recording the code it asked to exit with.
    ///
    /// Only the first request carries a code: a Cmd+Q pressed on top of a
    /// programmatic `exit(7)` must not turn that 7 into a 0.
    pub fn begin(&self, code: Option<i32>) -> QuitDecision {
        match self
            .phase
            .compare_exchange(IDLE, FLUSHING, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => {
                self.exit_code.store(code.unwrap_or(0), Ordering::SeqCst);
                QuitDecision::StartFlush
            }
            Err(FLUSHING) => QuitDecision::Wait,
            Err(_) => QuitDecision::Proceed,
        }
    }

    /// Marks the shutdown work finished. Call it after the last write lands,
    /// never before: a repeat request reads this to decide it may exit.
    pub fn finish(&self) {
        self.phase.store(COMPLETE, Ordering::SeqCst);
    }

    /// The code the exit should carry.
    pub fn exit_code(&self) -> i32 {
        self.exit_code.load(Ordering::SeqCst)
    }

    /// Takes up the shutdown work for a termination that never raised a
    /// request — on macOS, `NSApp terminate:` from the Dock or an Apple Event.
    ///
    /// Returns false when a request path already has the work, so the writes
    /// happen once whichever way the process is going down.
    pub fn claim_final_shutdown(&self) -> bool {
        self.phase
            .compare_exchange(IDLE, FLUSHING, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Whether the shutdown work has finished and the process may go.
    pub fn is_complete(&self) -> bool {
        self.phase.load(Ordering::SeqCst) == COMPLETE
    }

    /// Records that the frontend has flushed every pending save.
    ///
    /// Idempotent: the frontend may confirm twice, or after the wait has
    /// already given up, and neither changes what the exit path does next.
    pub fn confirm_flush(&self) {
        self.flush_confirmed.store(true, Ordering::SeqCst);
    }

    /// Blocks until the frontend confirms it has flushed, or until the wait
    /// runs out. Returns whether the confirmation arrived.
    ///
    /// Polls rather than parking on a condition variable: a wakeup that lands
    /// between the flag check and the wait is lost, and the one thing this
    /// must never do is outlive the timeout.
    pub fn wait_for_flush(&self) -> bool {
        let started = Instant::now();
        loop {
            let flushed = self.flush_confirmed.load(Ordering::SeqCst);
            if should_force_exit(started.elapsed(), flushed) {
                return flushed;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}
