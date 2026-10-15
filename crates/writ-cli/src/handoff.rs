//! Handing files to the running app through `<data dir>/open-requests/`, the
//! folder a second launch leaves its paths in (ADR-033), and waiting for their
//! tabs to close (ADR-017).
//!
//! A path carrying a line, and an open that waits, cannot travel through the
//! operating system's open-files path, which carries a path and nothing else.
//! They travel as an [`OpenRequest`] instead, which the app authorizes and
//! resolves path by path exactly as it does a launch argument. A plain open
//! still goes the operating system's way, as `writ <path>` always has.
//!
//! The answer to a wait is the removal of an empty marker this process left in
//! `<data dir>/waits/`. Nothing listens for it at rest: this process looks
//! while it waits, and stops looking when the marker is gone or the app holding
//! the data folder has ended.
//!
//! `get_context` asks the same way (ADR-053): a request carrying a context ask,
//! answered by a file the app leaves in `<data dir>/context-answers/`, which
//! this process looks for only while that one call waits. So does a write held
//! for the person's answer: the proposal waits in `write-reviews/`, and the
//! answer lands in `review-answers/`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use writ_core::instance::{ContextAsk, OpenRequest, ReviewAsk, OPEN_REQUEST_FRESH_MS};
use writ_core::open_context::OpenContext;
use writ_core::write_review::{ReviewAnswer, ReviewUnavailable, WriteProposal};
use writ_storage::instance::{
    acquire, create_wait_marker, new_wait_token, send_request, InstanceLockError,
};
use writ_storage::open_context::{take_answer, withdraw_answer};
use writ_storage::write_review;

use crate::verbs::OpenPlan;

/// How long a running app has to take a request before the open is called
/// unanswered.
pub const TAKE_WAIT_RUNNING: Duration = Duration::from_secs(10);

/// How long an app this command started has to take a request: as long as a
/// request stays worth acting on.
pub const TAKE_WAIT_LAUNCHED: Duration = Duration::from_millis(OPEN_REQUEST_FRESH_MS as u64);

/// How often the command looks while it waits.
pub const POLL: Duration = Duration::from_millis(100);

/// How long each step of a hand-over may take.
#[derive(Debug, Clone, Copy)]
pub struct Timings {
    /// For a request to a running app to be taken.
    pub running: Duration,
    /// For a request to an app this command started to be taken.
    pub launched: Duration,
    /// Between two looks.
    pub poll: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            running: TAKE_WAIT_RUNNING,
            launched: TAKE_WAIT_LAUNCHED,
            poll: POLL,
        }
    }
}

/// What came of handing files to the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivered {
    /// The app has the files.
    Opened,
    /// The app had the files and every tab they opened has closed.
    Closed,
    /// The app had the files and has ended since, closing their tabs.
    AppEnded,
}

/// Why nothing was handed over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffError {
    /// A path the request would carry is not UTF-8, which the request cannot
    /// spell.
    NotUtf8(PathBuf),
    /// The request or the marker could not be written.
    NotSent(String),
    /// The app did not take the request in time. It was withdrawn.
    Unanswered,
}

impl std::fmt::Display for HandoffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HandoffError::NotUtf8(path) => write!(
                f,
                "{} cannot be opened at a line or waited for, because its name is not UTF-8",
                path.display()
            ),
            HandoffError::NotSent(reason) => write!(f, "could not reach Writ: {reason}"),
            HandoffError::Unanswered => write!(f, "Writ did not answer, so nothing was opened"),
        }
    }
}

/// Whether an app holds the data folder at `writ_dir`.
///
/// Asked by trying the lock the app holds for its whole life
/// ([`writ_storage::instance::acquire`]) and letting go at once. A launch
/// that meets the lock in that instant waits for it and takes it, so the look
/// costs nothing. A folder that cannot be locked at all answers `false`: the
/// app then runs without the lock too, and starting it again only brings it
/// forward.
pub fn is_app_running(writ_dir: &Path) -> bool {
    matches!(acquire(writ_dir), Err(InstanceLockError::Held))
}

/// Opens `plan` in the app holding `writ_dir`, starting it through `launch`
/// when none does.
///
/// With nothing but paths to open, `launch` is handed the paths and the
/// operating system's open-files path carries them, as `writ <path>` always
/// has. Otherwise the request goes to the folder the app takes requests from,
/// and `launch` is handed nothing, and only when no app holds the folder: the
/// app it starts takes the request on its first look, and a running app
/// brings itself forward when it takes one.
///
/// # Errors
///
/// [`HandoffError`]: nothing was opened, and no request or marker is left.
pub fn deliver(
    writ_dir: &Path,
    plan: &OpenPlan,
    launch: impl FnOnce(&[PathBuf]),
    timings: Timings,
) -> Result<Delivered, HandoffError> {
    if plan.lines.is_empty() && !plan.wait {
        launch(&plan.paths);
        return Ok(Delivered::Opened);
    }
    let running = is_app_running(writ_dir);

    let mut paths = Vec::with_capacity(plan.paths.len());
    for path in &plan.paths {
        let spelled = path
            .to_str()
            .ok_or_else(|| HandoffError::NotUtf8(path.clone()))?;
        paths.push(spelled.to_string());
    }
    let wait = if plan.wait {
        let token = new_wait_token();
        let marker = create_wait_marker(writ_dir, &token)
            .map_err(|error| HandoffError::NotSent(error.to_string()))?;
        Some((token, marker))
    } else {
        None
    };
    let request = OpenRequest {
        paths,
        sent_at_ms: now_ms(),
        lines: plan.lines.clone(),
        wait: wait.as_ref().map(|(token, _)| token.clone()),
        links: Vec::new(),
        context: None,
        review: None,
    };
    let withdraw_marker = |wait: &Option<(String, PathBuf)>| {
        if let Some((_, marker)) = wait {
            let _ = std::fs::remove_file(marker);
        }
    };
    let sent = match send_request(writ_dir, &request) {
        Ok(sent) => sent,
        Err(error) => {
            withdraw_marker(&wait);
            return Err(HandoffError::NotSent(error.to_string()));
        }
    };
    if !running {
        launch(&[]);
    }
    let allowed = if running {
        timings.running
    } else {
        timings.launched
    };
    if !is_taken(&sent, allowed, timings.poll) {
        let _ = std::fs::remove_file(&sent);
        withdraw_marker(&wait);
        return Err(HandoffError::Unanswered);
    }
    match wait {
        None => Ok(Delivered::Opened),
        Some((_, marker)) => Ok(wait_for_close(writ_dir, &marker, timings.poll)),
    }
}

/// Hands the `writ://` link `link` to the app holding `writ_dir`, starting it
/// through `launch` when none does, and returns once the app has taken it.
///
/// The link travels as a request and the app reads it through the same
/// grammar as a clicked link (ADR-052), so `writ mcp` can ask for nothing a
/// link on a web page could not. `launch` answers whether it started the app.
///
/// # Errors
///
/// [`HandoffError`]: the link was not taken, and no request is left.
pub fn deliver_link(
    writ_dir: &Path,
    link: &str,
    launch: impl FnOnce() -> bool,
    timings: Timings,
) -> Result<(), HandoffError> {
    let running = is_app_running(writ_dir);
    let request = OpenRequest {
        sent_at_ms: now_ms(),
        links: vec![link.to_string()],
        ..OpenRequest::default()
    };
    let sent = send_request(writ_dir, &request)
        .map_err(|error| HandoffError::NotSent(error.to_string()))?;
    if !running && !launch() {
        let _ = std::fs::remove_file(&sent);
        return Err(HandoffError::NotSent(
            "Writ could not be started".to_string(),
        ));
    }
    let allowed = if running {
        timings.running
    } else {
        timings.launched
    };
    if !is_taken(&sent, allowed, timings.poll) {
        let _ = std::fs::remove_file(&sent);
        return Err(HandoffError::Unanswered);
    }
    Ok(())
}

/// Asks the app holding `writ_dir` which file is open and what is selected in
/// it, for `program`, and returns its answer.
///
/// Answers [`OpenContext::nothing_open`] at once when no app holds the folder:
/// a Writ that is not running has no file open, and this never starts one.
/// Otherwise the ask waits in `open-requests/` and its answer is looked for
/// every `poll` until `allowed` has passed.
///
/// # Errors
///
/// [`HandoffError`]: the ask could not be written, or the app did not answer
/// in time. The ask and any answer that lands later are removed.
pub fn ask_context(
    writ_dir: &Path,
    program: &str,
    allowed: Duration,
    poll: Duration,
) -> Result<OpenContext, HandoffError> {
    if !is_app_running(writ_dir) {
        return Ok(OpenContext::nothing_open());
    }
    let token = new_wait_token();
    let request = OpenRequest {
        sent_at_ms: now_ms(),
        context: Some(ContextAsk {
            token: token.clone(),
            program: program.to_string(),
        }),
        ..OpenRequest::default()
    };
    let sent = send_request(writ_dir, &request)
        .map_err(|error| HandoffError::NotSent(error.to_string()))?;
    let deadline = Instant::now() + allowed;
    loop {
        if let Some(answer) = take_answer(writ_dir, &token) {
            return Ok(answer);
        }
        if Instant::now() >= deadline {
            let _ = std::fs::remove_file(&sent);
            withdraw_answer(writ_dir, &token);
            return Err(HandoffError::Unanswered);
        }
        std::thread::sleep(poll);
    }
}

/// Shows `proposal` in the app holding `writ_dir` and returns the person's
/// answer.
///
/// [`ReviewUnavailable::NotRunning`] at once when no app holds the folder:
/// this never starts one. Otherwise the proposal waits in `write-reviews/`
/// beside a request carrying its token, and the answer is looked for every
/// `poll` until `allowed` has passed or the app ends. Whatever happens, the
/// request, the proposal and any answer are gone when this returns.
///
/// # Errors
///
/// [`ReviewUnavailable`]: no app to ask, the proposal could not be left, or
/// nobody answered in time.
pub fn ask_review(
    writ_dir: &Path,
    proposal: &WriteProposal,
    allowed: Duration,
    poll: Duration,
) -> Result<ReviewAnswer, ReviewUnavailable> {
    if !is_app_running(writ_dir) {
        return Err(ReviewUnavailable::NotRunning);
    }
    let token = new_wait_token();
    write_review::write_proposal(writ_dir, &token, proposal)
        .map_err(|_| ReviewUnavailable::NotShown)?;
    let request = OpenRequest {
        sent_at_ms: now_ms(),
        review: Some(ReviewAsk {
            token: token.clone(),
            program: proposal.program.clone(),
        }),
        ..OpenRequest::default()
    };
    let sent = match send_request(writ_dir, &request) {
        Ok(sent) => sent,
        Err(_) => {
            write_review::withdraw(writ_dir, &token);
            return Err(ReviewUnavailable::NotShown);
        }
    };
    let deadline = Instant::now() + allowed;
    let outcome = loop {
        if let Some(answer) = write_review::take_answer(writ_dir, &token) {
            break Ok(answer);
        }
        if Instant::now() >= deadline {
            break Err(ReviewUnavailable::Unanswered);
        }
        if !is_app_running(writ_dir) {
            break Err(ReviewUnavailable::NotRunning);
        }
        std::thread::sleep(poll);
    };
    let _ = std::fs::remove_file(&sent);
    write_review::withdraw(writ_dir, &token);
    outcome
}

/// Whether the request at `sent` is taken within `allowed`.
fn is_taken(sent: &Path, allowed: Duration, poll: Duration) -> bool {
    let deadline = Instant::now() + allowed;
    loop {
        if !sent.exists() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(poll);
    }
}

/// Returns once the app removes `marker`, or once no app holds `writ_dir`.
///
/// An app that ended has closed every tab it had, which is what the wait was
/// for; the marker it never removed is removed here.
pub fn wait_for_close(writ_dir: &Path, marker: &Path, poll: Duration) -> Delivered {
    loop {
        if !marker.exists() {
            return Delivered::Closed;
        }
        if !is_app_running(writ_dir) {
            let _ = std::fs::remove_file(marker);
            return Delivered::AppEnded;
        }
        std::thread::sleep(poll);
    }
}

/// Milliseconds since the Unix epoch, the stamp a request carries.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| i64::try_from(since.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use writ_storage::instance::{clear_waits, release_wait, take_requests, waits_dir};

    fn quick() -> Timings {
        Timings {
            running: Duration::from_secs(5),
            launched: Duration::from_millis(200),
            poll: Duration::from_millis(10),
        }
    }

    fn plan(paths: &[&str], line: Option<u32>, wait: bool) -> OpenPlan {
        let mut plan = OpenPlan {
            paths: paths.iter().map(PathBuf::from).collect(),
            wait,
            ..OpenPlan::default()
        };
        if let (Some(line), Some(first)) = (line, paths.first()) {
            plan.lines.insert((*first).to_string(), line);
        }
        plan
    }

    /// Plays the running app: takes the first request that arrives and hands
    /// it to `answer`.
    fn app(writ_dir: PathBuf, answer: impl FnOnce(&Path, OpenRequest) + Send + 'static) {
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if let Some(request) = take_requests(&writ_dir).into_iter().next() {
                    answer(&writ_dir, request);
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
    }

    #[test]
    fn with_no_app_running_plain_paths_go_to_the_launcher() {
        let dir = tempfile::tempdir().expect("tempdir");
        let launched = RefCell::new(None);
        let delivered = deliver(
            dir.path(),
            &plan(&["/notes/a.md"], None, false),
            |paths| *launched.borrow_mut() = Some(paths.to_vec()),
            quick(),
        );
        assert_eq!(delivered, Ok(Delivered::Opened));
        assert_eq!(
            launched.into_inner(),
            Some(vec![PathBuf::from("/notes/a.md")])
        );
        assert!(take_requests(dir.path()).is_empty());
    }

    #[test]
    fn plain_paths_go_to_the_launcher_while_the_app_runs_too() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _app = acquire(dir.path()).expect("the app holds the folder");
        let launched = RefCell::new(None);
        let delivered = deliver(
            dir.path(),
            &plan(&["/notes/a.md"], None, false),
            |paths| *launched.borrow_mut() = Some(paths.to_vec()),
            quick(),
        );
        assert_eq!(delivered, Ok(Delivered::Opened));
        assert_eq!(
            launched.into_inner(),
            Some(vec![PathBuf::from("/notes/a.md")])
        );
        assert!(take_requests(dir.path()).is_empty());
    }

    #[test]
    fn a_running_app_is_handed_the_request_and_nothing_is_launched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _app = acquire(dir.path()).expect("the app holds the folder");
        let (tx, rx) = std::sync::mpsc::channel();
        app(dir.path().to_path_buf(), move |_, request| {
            tx.send(request).expect("send");
        });
        let delivered = deliver(
            dir.path(),
            &plan(&["/notes/a.md"], Some(12), false),
            |_| panic!("nothing is launched while the app runs"),
            quick(),
        );
        assert_eq!(delivered, Ok(Delivered::Opened));
        let request = rx.recv().expect("taken");
        assert_eq!(request.paths, vec!["/notes/a.md".to_string()]);
        assert_eq!(request.lines.get("/notes/a.md"), Some(&12));
        assert_eq!(request.wait, None);
    }

    #[test]
    fn a_wait_returns_when_the_app_answers_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _app = acquire(dir.path()).expect("the app holds the folder");
        app(dir.path().to_path_buf(), |writ_dir, request| {
            std::thread::sleep(Duration::from_millis(50));
            release_wait(writ_dir, request.wait_token().expect("a token"));
        });
        let delivered = deliver(
            dir.path(),
            &plan(&["/notes/msg.txt"], None, true),
            |_| panic!("nothing is launched while the app runs"),
            quick(),
        );
        assert_eq!(delivered, Ok(Delivered::Closed));
        assert_eq!(
            std::fs::read_dir(waits_dir(dir.path()))
                .expect("waits")
                .count(),
            0
        );
    }

    #[test]
    fn a_wait_returns_when_the_app_ends() {
        let dir = tempfile::tempdir().expect("tempdir");
        let held = acquire(dir.path()).expect("the app holds the folder");
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        app(dir.path().to_path_buf(), move |_, _| {
            tx.send(()).expect("send");
        });
        let ender = std::thread::spawn(move || {
            rx.recv().expect("taken");
            std::thread::sleep(Duration::from_millis(50));
            drop(held);
        });
        let delivered = deliver(
            dir.path(),
            &plan(&["/notes/msg.txt"], None, true),
            |_| panic!("nothing is launched while the app runs"),
            quick(),
        );
        ender.join().expect("ender");
        assert_eq!(delivered, Ok(Delivered::AppEnded));
        assert_eq!(
            std::fs::read_dir(waits_dir(dir.path()))
                .expect("waits")
                .count(),
            0
        );
    }

    #[test]
    fn with_no_app_running_a_line_starts_it_empty_handed_and_leaves_the_request() {
        let dir = tempfile::tempdir().expect("tempdir");
        let launched = RefCell::new(None);
        let writ_dir = dir.path().to_path_buf();
        let delivered = deliver(
            dir.path(),
            &plan(&["/notes/a.md"], Some(3), false),
            |paths| {
                *launched.borrow_mut() = Some(paths.to_vec());
                // The app this started takes the request on its first look.
                let request = take_requests(&writ_dir).into_iter().next();
                assert_eq!(
                    request.and_then(|r| r.lines.get("/notes/a.md").copied()),
                    Some(3)
                );
            },
            quick(),
        );
        assert_eq!(delivered, Ok(Delivered::Opened));
        assert_eq!(launched.into_inner(), Some(Vec::new()));
    }

    #[test]
    fn a_wait_that_starts_the_app_lasts_until_the_tab_closes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let writ_dir = dir.path().to_path_buf();
        let tab_open_for = Duration::from_millis(200);
        let started = Instant::now();
        let delivered = deliver(
            dir.path(),
            &plan(&["/notes/msg.txt"], None, true),
            |_| {
                // A cold start as the app makes it: claim the folder, clear
                // what an earlier run left, then take the request and close
                // the tab a moment later.
                let writ_dir = writ_dir.clone();
                std::thread::spawn(move || {
                    let _lock = acquire(&writ_dir).expect("the app claims the folder");
                    clear_waits(&writ_dir);
                    let request = take_requests(&writ_dir)
                        .into_iter()
                        .next()
                        .expect("the request");
                    std::thread::sleep(tab_open_for);
                    release_wait(&writ_dir, request.wait_token().expect("a token"));
                    std::thread::sleep(Duration::from_millis(100));
                });
            },
            quick(),
        );
        assert_eq!(delivered, Ok(Delivered::Closed));
        assert!(
            started.elapsed() >= tab_open_for,
            "the wait ended before the tab closed"
        );
    }

    #[test]
    fn an_unanswered_request_is_withdrawn_with_its_marker() {
        let dir = tempfile::tempdir().expect("tempdir");
        let delivered = deliver(
            dir.path(),
            &plan(&["/notes/a.md"], None, true),
            |_| {},
            quick(),
        );
        assert_eq!(delivered, Err(HandoffError::Unanswered));
        assert!(take_requests(dir.path()).is_empty());
        assert_eq!(
            std::fs::read_dir(waits_dir(dir.path()))
                .expect("waits")
                .count(),
            0
        );
    }

    #[test]
    fn a_link_is_handed_to_the_running_app_as_it_was_given() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _app = acquire(dir.path()).expect("the app holds the folder");
        let (tx, rx) = std::sync::mpsc::channel();
        app(dir.path().to_path_buf(), move |_, request| {
            tx.send(request).expect("send");
        });
        let link = "writ://open?path=Plan.md&line=12";
        let delivered = deliver_link(
            dir.path(),
            link,
            || panic!("nothing is launched while the app runs"),
            quick(),
        );
        assert_eq!(delivered, Ok(()));
        let request = rx.recv().expect("taken");
        assert_eq!(request.links, vec![link.to_string()]);
        assert!(request.paths.is_empty());
    }

    #[test]
    fn a_link_starts_the_app_when_none_holds_the_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let launched = RefCell::new(false);
        let writ_dir = dir.path().to_path_buf();
        let delivered = deliver_link(
            dir.path(),
            "writ://open?path=Plan.md",
            || {
                *launched.borrow_mut() = true;
                app(writ_dir.clone(), |_, _| {});
                true
            },
            quick(),
        );
        assert_eq!(delivered, Ok(()));
        assert!(launched.into_inner());
    }

    #[test]
    fn a_link_nobody_takes_is_withdrawn() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            deliver_link(dir.path(), "writ://open?path=Plan.md", || false, quick()),
            Err(HandoffError::NotSent(
                "Writ could not be started".to_string()
            ))
        );
        assert!(take_requests(dir.path()).is_empty());

        let _app = acquire(dir.path()).expect("an app that never answers");
        let slow = Timings {
            running: Duration::from_millis(50),
            ..quick()
        };
        assert_eq!(
            deliver_link(dir.path(), "writ://open?path=Plan.md", || true, slow),
            Err(HandoffError::Unanswered)
        );
        assert!(take_requests(dir.path()).is_empty());
    }

    #[test]
    fn with_no_app_running_nothing_is_open_and_nothing_is_asked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let answer = ask_context(
            dir.path(),
            "Claude Code",
            Duration::from_secs(1),
            Duration::from_millis(5),
        );
        assert_eq!(answer, Ok(OpenContext::nothing_open()));
        assert!(take_requests(dir.path()).is_empty());
    }

    #[test]
    fn a_running_app_is_asked_for_the_open_file_and_its_answer_is_taken_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _app = acquire(dir.path()).expect("the test plays the running app");
        let open = OpenContext {
            path: Some("/notes/Plan.md".to_string()),
            unsaved: true,
            ..OpenContext::default()
        };
        let written = open.clone();
        app(dir.path().to_path_buf(), move |writ_dir, request| {
            assert!(request.paths.is_empty() && request.links.is_empty());
            let ask = request.context.expect("a context ask");
            assert_eq!(ask.program, "Claude Code");
            writ_storage::open_context::write_answer(writ_dir, &ask.token, &written)
                .expect("answer");
        });

        let answer = ask_context(
            dir.path(),
            "Claude Code",
            Duration::from_secs(5),
            Duration::from_millis(5),
        );

        assert_eq!(answer, Ok(open));
        let left = std::fs::read_dir(writ_storage::open_context::answers_dir(dir.path()))
            .expect("the answers folder")
            .count();
        assert_eq!(left, 0, "the answer is removed once read");
    }

    fn proposal() -> WriteProposal {
        WriteProposal {
            program: "claude-code".to_string(),
            path: "/notes/Plan.md".to_string(),
            change: writ_core::write_review::ProposedChange::Edit {
                before: "one\n".to_string(),
                after: "one\ntwo\n".to_string(),
            },
        }
    }

    fn left_in(dir: PathBuf) -> usize {
        std::fs::read_dir(dir).map_or(0, |entries| entries.count())
    }

    #[test]
    fn with_no_app_running_a_held_write_asks_nobody() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            ask_review(
                dir.path(),
                &proposal(),
                Duration::from_secs(1),
                Duration::from_millis(5)
            ),
            Err(ReviewUnavailable::NotRunning)
        );
        assert!(take_requests(dir.path()).is_empty());
        assert_eq!(left_in(write_review::proposals_dir(dir.path())), 0);
    }

    #[test]
    fn a_running_app_is_shown_the_change_and_its_answer_is_returned() {
        for given in [ReviewAnswer::Apply, ReviewAnswer::Discard] {
            let dir = tempfile::tempdir().expect("tempdir");
            let _app = acquire(dir.path()).expect("the test plays the running app");
            app(dir.path().to_path_buf(), move |writ_dir, request| {
                assert!(request.paths.is_empty() && request.context.is_none());
                assert!(!request.shows_the_window());
                let ask = request.review.expect("a review ask");
                assert_eq!(ask.program, "claude-code");
                assert_eq!(
                    write_review::read_proposal(writ_dir, &ask.token),
                    Some(proposal()),
                    "the app reads what would change"
                );
                write_review::write_answer(writ_dir, &ask.token, given).expect("answer");
            });

            let answer = ask_review(
                dir.path(),
                &proposal(),
                Duration::from_secs(5),
                Duration::from_millis(5),
            );

            assert_eq!(answer, Ok(given));
            assert_eq!(left_in(write_review::proposals_dir(dir.path())), 0);
            assert_eq!(left_in(write_review::answers_dir(dir.path())), 0);
        }
    }

    #[test]
    fn a_held_write_nobody_answers_is_withdrawn() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _app = acquire(dir.path()).expect("the test plays the running app");
        let answer = ask_review(
            dir.path(),
            &proposal(),
            Duration::from_millis(100),
            Duration::from_millis(5),
        );
        assert_eq!(answer, Err(ReviewUnavailable::Unanswered));
        assert!(take_requests(dir.path()).is_empty());
        assert_eq!(left_in(write_review::proposals_dir(dir.path())), 0);
    }

    #[test]
    fn a_held_write_stops_waiting_when_the_app_ends() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_lock = acquire(dir.path()).expect("the test plays the running app");
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            drop(app_lock);
        });
        let answer = ask_review(
            dir.path(),
            &proposal(),
            Duration::from_secs(5),
            Duration::from_millis(5),
        );
        assert_eq!(answer, Err(ReviewUnavailable::NotRunning));
    }

    #[test]
    fn an_app_that_does_not_answer_leaves_no_ask_behind() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _app = acquire(dir.path()).expect("the test plays the running app");
        let answer = ask_context(
            dir.path(),
            "Claude Code",
            Duration::from_millis(100),
            Duration::from_millis(5),
        );
        assert_eq!(answer, Err(HandoffError::Unanswered));
        assert!(take_requests(dir.path()).is_empty());
    }
}
