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

use std::collections::BTreeMap;
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

/// Leaves `paths` and `links` for the process holding `writ_dir` and waits up
/// to `wait` for them to be taken, claiming the folder instead if its holder
/// exits first.
pub fn hand_over(
    writ_dir: &Path,
    paths: Vec<String>,
    links: Vec<String>,
    wait: Duration,
    poll: Duration,
) -> Handover {
    let request = OpenRequest {
        paths,
        sent_at_ms: now_ms(),
        links,
        ..OpenRequest::default()
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
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let links = writ_core::writ_link::link_args(args.iter().cloned());
    let paths = launch_paths(args);
    match hand_over(writ_dir, paths, links, HANDOVER_WAIT, HANDOVER_POLL) {
        Handover::TookOver(lock) => Ok(lock),
        Handover::Delivered => std::process::exit(0),
        Handover::Unanswered => {
            eprintln!("writ: the running Writ did not answer; it opens the files when it can");
            std::process::exit(0)
        }
        Handover::NotSent => std::process::exit(1),
    }
}

/// What a forwarded request asks the frontend to open, as authorized and
/// resolved paths with the line each one carries.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ForwardedOpens {
    /// The paths, resolved the way a launch argument is.
    pub paths: Vec<String>,
    /// The line to show for a path, keyed by the resolved path.
    pub lines: BTreeMap<String, u32>,
}

/// Authorizes and resolves each path of `request` as a launch argument is,
/// keeping its line beside the path it resolved to.
///
/// Each path is resolved on its own, so a refused path drops out with its
/// line rather than handing its line to the path after it.
pub fn authorize_request(state: &AppState, request: &OpenRequest) -> ForwardedOpens {
    let mut opens = ForwardedOpens::default();
    for raw in &request.paths {
        let resolved = crate::startup::authorize_and_canonicalize(
            &state.authorized_paths,
            std::slice::from_ref(raw),
        );
        let Some(path) = resolved.into_iter().next() else {
            continue;
        };
        if let Some(line) = request.lines.get(raw) {
            opens.lines.insert(path.clone(), *line);
        }
        opens.paths.push(path);
    }
    opens
}

/// Records the `writ --wait` a request carries against the files it opens,
/// answering it at once when none of them could be opened.
pub fn register_wait(state: &AppState, request: &OpenRequest, opens: &ForwardedOpens) {
    let Some(token) = request.wait_token() else {
        return;
    };
    let answer_now = recover_poison(state.cli_waits.lock(), "instance::register_wait")
        .register(token, opens.paths.iter().map(|path| wait_key(path)));
    if answer_now {
        writ_storage::instance::release_wait(&state.writ_dir, token);
    }
}

/// A path as the waits compare it: resolved the way an open resolves it, so a
/// tab's file and a request's path name one file the same way on every
/// platform.
fn wait_key(path: &str) -> String {
    crate::security::canonicalize_for_authorization(Path::new(path))
        .unwrap_or_else(|_| path.to_string())
}

/// Answers every `writ --wait` whose last file is among `closed`, the files
/// whose last tab has just closed.
///
/// Called after the close has let go of the store and the note's turn, so the
/// file's last write has landed before the waiting command returns.
pub fn tabs_closed(state: &AppState, closed: &[String]) {
    if closed.is_empty()
        || recover_poison(state.cli_waits.lock(), "instance::tabs_closed").is_empty()
    {
        return;
    }
    // Resolved outside the lock: resolving asks the disk.
    let keys: Vec<String> = closed.iter().map(|path| wait_key(path)).collect();
    let answered = recover_poison(state.cli_waits.lock(), "instance::tabs_closed")
        .closed(keys.iter().map(String::as_str));
    for token in answered {
        info!("a waiting writ command was answered");
        writ_storage::instance::release_wait(&state.writ_dir, &token);
    }
}

/// The opens of a forwarded request the frontend should be told about now, or
/// `None` when there is nothing to tell yet.
///
/// Each path is authorized and resolved as a launch argument is
/// ([`authorize_request`]). Before the frontend is ready they are parked with
/// the rest of the launch's opens, their lines beside them, and delivered when
/// it is.
pub fn route_forwarded(state: &AppState, request: &OpenRequest) -> Option<ForwardedOpens> {
    if request.paths.is_empty() {
        return None;
    }
    let opens = authorize_request(state, request);
    register_wait(state, request, &opens);
    if opens.paths.is_empty() {
        return None;
    }
    info!(
        count = opens.paths.len(),
        "files forwarded from a second launch"
    );
    if state.frontend_ready.load(Ordering::SeqCst) {
        return Some(opens);
    }
    let mut pending = recover_poison(state.pending_opens.lock(), "instance::route_forwarded");
    pending.extend(opens.paths);
    drop(pending);
    recover_poison(state.pending_lines.lock(), "instance::route_forwarded").extend(opens.lines);
    None
}

/// Opens what a second launch or a `writ` command asked for, follows the
/// `writ://` links it carries, and brings the window forward.
pub fn deliver_forwarded(app: &AppHandle, request: &OpenRequest) {
    let state = app.state::<AppState>();
    if let Some(opens) = route_forwarded(&state, request) {
        let _ = emit_event(
            app,
            WritFrontendEvent::PendingOpens {
                paths: opens.paths,
                lines: opens.lines,
            },
        );
    }
    crate::links::follow(app, &request.links, false);
    bring_forward(app);
}

/// Shows the main window, out of the Dock or the taskbar, and focuses it.
pub fn bring_forward(app: &AppHandle) {
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
    mut deliver: impl FnMut(OpenRequest),
) -> usize {
    if leaving {
        return 0;
    }
    let mut delivered = 0;
    for request in take_requests(writ_dir) {
        if !request.is_fresh(now_ms) {
            info!("an open request older than a minute was dropped");
            if let Some(token) = request.wait_token() {
                writ_storage::instance::release_wait(writ_dir, token);
            }
            continue;
        }
        deliver(request);
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
            drain_requests(&writ_dir, now_ms(), leaving, |request| {
                if let Some(ask) = request.context_ask(now_ms()) {
                    crate::open_context::answer_ask(app, ask);
                }
                if let Some(ask) = request.review_ask(now_ms()) {
                    crate::write_review::show_review(app, ask, request.sent_at_ms, now_ms());
                }
                if request.shows_the_window() {
                    deliver_forwarded(app, &request);
                }
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
            Vec::new(),
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
            Vec::new(),
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
            Vec::new(),
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
            ..OpenRequest::default()
        };
        send_request(dir.path(), &request).expect("send");
        let mut delivered = Vec::new();
        assert_eq!(
            drain_requests(dir.path(), 1_000, true, |r| delivered.push(r.paths)),
            0
        );
        assert!(delivered.is_empty());
        assert_eq!(
            drain_requests(dir.path(), 1_000, false, |r| delivered.push(r.paths)),
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
            ..OpenRequest::default()
        };
        send_request(dir.path(), &request).expect("send");
        let mut delivered = Vec::new();
        let now = writ_core::instance::OPEN_REQUEST_FRESH_MS + 1;
        assert_eq!(
            drain_requests(dir.path(), now, false, |r| delivered.push(r.paths)),
            0
        );
        assert!(delivered.is_empty());
        assert!(
            take_requests(dir.path()).is_empty(),
            "a stale request is cleared"
        );
    }
}
