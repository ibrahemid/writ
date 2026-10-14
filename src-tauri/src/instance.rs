//! One Writ per data folder: what a second launch does, and how the running
//! one hears it (ADR-033, amended 2026-10-14).
//!
//! `run` claims the folder before anything opens the database
//! ([`writ_storage::instance::acquire`]). A launch that finds it held leaves
//! its paths as a request and waits for the running instance to take it. A
//! relaunch races the process it replaces, which still holds the folder while
//! it exits, so the wait also watches the lock: the moment it comes free the
//! waiting launch claims it and starts as the running instance itself. A
//! process that is quitting stops taking requests for the same reason.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use tauri::{AppHandle, Manager};
use tracing::{error, info};
use writ_core::instance::OpenRequest;
use writ_storage::instance::{
    acquire, requests_dir, send_request, take_requests, InstanceLock, InstanceLockError,
};

use crate::events::{emit_event, WritFrontendEvent};
use crate::poison::recover_poison;
use crate::state::AppState;

/// How long a second launch waits for the running instance to take its
/// request before it gives up and exits.
pub const HANDOVER_WAIT: Duration = Duration::from_secs(5);

/// How often the waiting launch looks.
pub const HANDOVER_POLL: Duration = Duration::from_millis(50);

/// How long the running instance's watcher coalesces request files.
const REQUEST_DEBOUNCE: Duration = Duration::from_millis(100);

/// What came of a launch that found the folder held.
#[derive(Debug)]
pub enum Handover {
    /// The running instance took the request.
    Delivered,
    /// The holder exited while this launch waited, and this launch now holds
    /// the folder: it starts as the running instance.
    TookOver(InstanceLock),
    /// Nothing took the request in time. It stays on disk for the running
    /// instance to find while it is still fresh.
    Unanswered,
    /// The request could not be written.
    NotSent,
}

/// Milliseconds since the Unix epoch, the stamp a request carries.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or(0)
}

/// The files a launch was given, resolved against this process's working
/// folder ([`writ_core::file_ops::arg_paths_from_iter`]): the running instance
/// has a working folder of its own.
pub fn launch_paths<I>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = OsString>,
{
    writ_core::file_ops::arg_paths_from_iter(args)
        .into_iter()
        .filter_map(|path| path.to_str().map(String::from))
        .collect()
}

/// Leaves `paths` for the process holding `writ_dir` and waits up to `wait`
/// for it to be taken, claiming the folder instead if its holder exits first.
pub fn hand_over(writ_dir: &Path, paths: Vec<String>, wait: Duration, poll: Duration) -> Handover {
    let request = OpenRequest {
        paths,
        sent_at_ms: now_ms(),
    };
    let sent = match send_request(writ_dir, &request) {
        Ok(sent) => sent,
        Err(error) => {
            eprintln!("writ: could not reach the running Writ: {error}");
            return Handover::NotSent;
        }
    };
    let deadline = Instant::now() + wait;
    loop {
        if !sent.exists() {
            return Handover::Delivered;
        }
        if let Ok(lock) = acquire(writ_dir) {
            // Taken by nobody who is still running: this process opens the
            // paths itself, as the instance it now is.
            let _ = std::fs::remove_file(&sent);
            return Handover::TookOver(lock);
        }
        if Instant::now() >= deadline {
            return Handover::Unanswered;
        }
        std::thread::sleep(poll);
    }
}

/// Claims `writ_dir` for this launch, or hands its arguments to the instance
/// holding it and ends the process.
///
/// Returns the lock to hold for the life of the process, or `None` with the
/// reason when the folder cannot be locked at all (a filesystem without
/// advisory locks): Writ then runs as it did before the lock existed, and the
/// caller logs why once logging is up.
pub fn claim_or_hand_over(writ_dir: &Path) -> Result<InstanceLock, InstanceLockError> {
    match acquire(writ_dir) {
        Err(InstanceLockError::Held) => {}
        claimed => return claimed,
    }
    let paths = launch_paths(std::env::args_os().skip(1));
    match hand_over(writ_dir, paths, HANDOVER_WAIT, HANDOVER_POLL) {
        Handover::TookOver(lock) => Ok(lock),
        Handover::Delivered => std::process::exit(0),
        Handover::Unanswered => {
            eprintln!("writ: the running Writ did not answer; it opens the files when it can");
            std::process::exit(0)
        }
        Handover::NotSent => std::process::exit(1),
    }
}

/// The paths of a forwarded request the frontend should be told about now, or
/// `None` when there is nothing to tell yet.
///
/// Each path is authorized and resolved as a launch argument is. Before the
/// frontend is ready they are parked with the rest of the launch's opens and
/// delivered when it is.
pub fn route_forwarded(state: &AppState, raw_paths: &[String]) -> Option<Vec<String>> {
    if raw_paths.is_empty() {
        return None;
    }
    let paths = crate::startup::authorize_and_canonicalize(&state.authorized_paths, raw_paths);
    if paths.is_empty() {
        return None;
    }
    info!(count = paths.len(), "files forwarded from a second launch");
    if state.frontend_ready.load(Ordering::SeqCst) {
        return Some(paths);
    }
    let mut pending = recover_poison(state.pending_opens.lock(), "instance::route_forwarded");
    pending.extend(paths);
    None
}

/// Opens what a second launch was given and brings the window forward.
pub fn deliver_forwarded(app: &AppHandle, raw_paths: &[String]) {
    let state = app.state::<AppState>();
    if let Some(paths) = route_forwarded(&state, raw_paths) {
        let _ = emit_event(app, WritFrontendEvent::PendingOpens { paths });
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Takes every request waiting in `writ_dir` and hands each fresh one to
/// `deliver`, returning how many were delivered.
///
/// A process that is `leaving` takes nothing: the launch waiting on its
/// request claims the folder once this one has gone, and opens the paths
/// itself.
pub fn drain_requests(
    writ_dir: &Path,
    now_ms: i64,
    leaving: bool,
    mut deliver: impl FnMut(Vec<String>),
) -> usize {
    if leaving {
        return 0;
    }
    let mut delivered = 0;
    for request in take_requests(writ_dir) {
        if !request.is_fresh(now_ms) {
            info!("an open request older than a minute was dropped");
            continue;
        }
        deliver(request.paths);
        delivered += 1;
    }
    delivered
}

/// Owner of the requests watcher. Held in Tauri's managed state so the watch
/// lives as long as the app.
pub struct OpenRequestWatcher {
    _debouncer: Debouncer<RecommendedWatcher>,
}

/// Watches `<writ_dir>/open-requests/` for a second launch's requests, and
/// takes what is already waiting there.
pub fn start_open_request_watcher(
    app: AppHandle,
    writ_dir: PathBuf,
) -> Result<OpenRequestWatcher, Box<dyn std::error::Error>> {
    let dir = requests_dir(&writ_dir);
    std::fs::create_dir_all(&dir)?;
    let (tx, rx) = mpsc::channel::<DebounceEventResult>();
    let mut debouncer = new_debouncer(REQUEST_DEBOUNCE, tx)?;
    debouncer
        .watcher()
        .watch(&dir, RecursiveMode::NonRecursive)?;

    std::thread::spawn(move || {
        let drain = |app: &AppHandle| {
            let leaving = app.state::<AppState>().quit.has_begun();
            drain_requests(&writ_dir, now_ms(), leaving, |paths| {
                deliver_forwarded(app, &paths)
            });
        };
        // Registered first, then read: a request written in between is in
        // the folder for this look, and anything later raises an event.
        drain(&app);
        while let Ok(result) = rx.recv() {
            match result {
                Ok(_) => drain(&app),
                Err(e) => error!("open request watcher error: {:?}", e),
            }
        }
    });

    Ok(OpenRequestWatcher {
        _debouncer: debouncer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use writ_storage::instance::{acquire, send_request};

    #[test]
    fn a_launch_finding_the_folder_held_is_answered_when_its_request_is_taken() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _running = acquire(dir.path()).expect("running instance");
        let writ_dir = dir.path().to_path_buf();
        let taker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if !take_requests(&writ_dir).is_empty() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let outcome = hand_over(
            dir.path(),
            vec!["/notes/a.md".to_string()],
            Duration::from_secs(5),
            Duration::from_millis(10),
        );
        taker.join().expect("taker");
        assert!(matches!(outcome, Handover::Delivered), "{outcome:?}");
    }

    #[test]
    fn a_launch_claims_the_folder_when_its_holder_exits_mid_wait() {
        let dir = tempfile::tempdir().expect("tempdir");
        let running = acquire(dir.path()).expect("running instance");
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            drop(running);
        });
        let outcome = hand_over(
            dir.path(),
            vec!["/notes/a.md".to_string()],
            Duration::from_secs(5),
            Duration::from_millis(10),
        );
        releaser.join().expect("releaser");
        assert!(matches!(outcome, Handover::TookOver(_)), "{outcome:?}");
        assert!(
            take_requests(dir.path()).is_empty(),
            "the launch that took over removes its own request"
        );
    }

    #[test]
    fn a_launch_nobody_answers_leaves_its_request_and_gives_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _running = acquire(dir.path()).expect("running instance");
        let outcome = hand_over(
            dir.path(),
            vec!["/notes/a.md".to_string()],
            Duration::from_millis(60),
            Duration::from_millis(10),
        );
        assert!(matches!(outcome, Handover::Unanswered), "{outcome:?}");
        assert_eq!(take_requests(dir.path()).len(), 1);
    }

    #[test]
    fn a_quitting_instance_takes_no_request() {
        let dir = tempfile::tempdir().expect("tempdir");
        let request = OpenRequest {
            paths: vec!["/notes/a.md".to_string()],
            sent_at_ms: 1_000,
        };
        send_request(dir.path(), &request).expect("send");
        let mut delivered = Vec::new();
        assert_eq!(
            drain_requests(dir.path(), 1_000, true, |p| delivered.push(p)),
            0
        );
        assert!(delivered.is_empty());
        assert_eq!(
            drain_requests(dir.path(), 1_000, false, |p| delivered.push(p)),
            1
        );
        assert_eq!(delivered, vec![vec!["/notes/a.md".to_string()]]);
    }

    #[test]
    fn a_stale_request_is_dropped_unopened() {
        let dir = tempfile::tempdir().expect("tempdir");
        let request = OpenRequest {
            paths: vec!["/notes/a.md".to_string()],
            sent_at_ms: 0,
        };
        send_request(dir.path(), &request).expect("send");
        let mut delivered = Vec::new();
        let now = writ_core::instance::OPEN_REQUEST_FRESH_MS + 1;
        assert_eq!(
            drain_requests(dir.path(), now, false, |p| delivered.push(p)),
            0
        );
        assert!(delivered.is_empty());
        assert!(
            take_requests(dir.path()).is_empty(),
            "a stale request is cleared"
        );
    }
}
