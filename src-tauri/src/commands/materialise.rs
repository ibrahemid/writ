//! Downloading a note whose bytes a sync provider has not put on this machine.
//!
//! The *policy* — that a placeholder is downloaded rather than read on the
//! caller's thread, and how long the wait is — lives in
//! [`writ_core::notes::materialise`]. This module is the *mechanism*: a worker
//! thread reads the file to the end, which is what asks the provider daemon
//! for the bytes, and the states that read passes through reach the frontend
//! as `writ://note-download` events.
//!
//! There is no provider API behind this. Reading the file is the request, so
//! the download cannot be stopped once it has started; cancelling stops Writ
//! waiting for it and drops the result, and nothing is opened.

use std::collections::HashMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, State};
use writ_core::notes::materialise::{DownloadOutcome, DOWNLOAD_TIMEOUT};

use crate::events::{emit_event, NoteDownloadState, WritFrontendEvent};
use crate::poison::recover_poison;
use crate::security::AuthorizedPaths;
use crate::state::AppState;

/// How often the wait wakes to look at the cancel flag and the clock.
///
/// The read runs on a thread of its own and cannot be interrupted, so the flag
/// is polled rather than awaited. Short enough that Cancel feels immediate,
/// long enough that a three-minute wait is not a spin.
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// How much of the file is read at a time. The bytes are discarded; only the
/// reading matters, and a fixed buffer keeps a large note off the heap.
const READ_CHUNK_BYTES: usize = 64 * 1024;

/// Downloads in flight, keyed by canonical path.
///
/// Managed separately from [`AppState`] for the same reason
/// [`crate::commands::ai::AiState`] is: it is session-scoped runtime state
/// with nothing persisted behind it.
#[derive(Default)]
pub struct MaterialiseState {
    /// Cancel flag per path being downloaded. A path is in the map for exactly
    /// as long as a worker is waiting on it, so its presence is what makes a
    /// second open join the download instead of starting a second one.
    in_flight: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl MaterialiseState {
    /// Registers `path` as in flight, or hands back the flag of the download
    /// already running for it.
    fn claim(&self, path: &str) -> Claim {
        let mut guard = recover_poison(self.in_flight.lock(), "materialise::claim");
        match guard.get(path) {
            Some(_) => Claim::Joined,
            None => {
                let cancel = Arc::new(AtomicBool::new(false));
                guard.insert(path.to_string(), Arc::clone(&cancel));
                Claim::Started(cancel)
            }
        }
    }

    /// Drops `path` from the map. Called on every outcome, the timeout
    /// included: a path left behind would make the next open join a download
    /// nothing is waiting on.
    fn release(&self, path: &str) {
        recover_poison(self.in_flight.lock(), "materialise::release").remove(path);
    }

    /// Raises the cancel flag of the download for `path`, if there is one.
    fn cancel(&self, path: &str) {
        let guard = recover_poison(self.in_flight.lock(), "materialise::cancel");
        if let Some(flag) = guard.get(path) {
            flag.store(true, Ordering::SeqCst);
        }
    }

    /// Whether a download is running for `path`.
    #[cfg(any(test, debug_assertions))]
    pub fn is_in_flight(&self, path: &str) -> bool {
        recover_poison(self.in_flight.lock(), "materialise::is_in_flight").contains_key(path)
    }
}

/// What [`MaterialiseState::claim`] answered.
enum Claim {
    /// This caller owns the download; here is its cancel flag.
    Started(Arc<AtomicBool>),
    /// A download for the path is already running; this caller waits on it.
    Joined,
}

/// Waits for `read` to finish, giving up when `cancel` is raised or `timeout`
/// passes, and reports what happened.
///
/// The read is what makes the provider fetch the file, and it cannot be
/// interrupted, so it runs on a thread of its own and this function only
/// decides how long to keep listening. A cancelled or timed-out read is
/// abandoned rather than stopped: its result lands on a channel nobody reads
/// and is dropped.
pub fn materialise_with<R, F>(
    path: PathBuf,
    cancel: Arc<AtomicBool>,
    timeout: Duration,
    read: R,
    mut report: F,
) where
    R: FnOnce(&Path) -> io::Result<()> + Send + 'static,
    F: FnMut(DownloadOutcome),
{
    let (tx, rx) = mpsc::channel::<io::Result<()>>();
    let read_path = path.clone();
    // Detached on purpose. A read waiting on a provider cannot be interrupted,
    // so this thread outlives a cancel or a timeout and ends whenever the
    // provider answers. By then the receiver is gone: the send fails, the
    // result is dropped, and nothing is emitted or opened on its behalf.
    std::thread::spawn(move || {
        let _ = tx.send(read(&read_path));
    });

    let started = Instant::now();
    loop {
        if cancel.load(Ordering::SeqCst) {
            report(DownloadOutcome::Cancelled);
            return;
        }
        let received = rx.recv_timeout(POLL_INTERVAL);
        // A cancel raised while the read was running is answered whatever the
        // read came back with. The flag going up before the outcome is
        // reported is the whole of what the person asked for, and a read that
        // finished inside the same poll window must not undo it.
        if cancel.load(Ordering::SeqCst) {
            report(DownloadOutcome::Cancelled);
            return;
        }
        match received {
            Ok(Ok(())) => {
                report(DownloadOutcome::Done);
                return;
            }
            Ok(Err(e)) => {
                report(DownloadOutcome::Failed(e.to_string()));
                return;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                report(DownloadOutcome::Failed(
                    "the download stopped without saying why".to_string(),
                ));
                return;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if started.elapsed() >= timeout {
                    report(DownloadOutcome::TimedOut);
                    return;
                }
            }
        }
    }
}

/// Reads `path` to the end and discards the bytes, then checks the file is
/// really here.
///
/// The read is the download: a provider materialises a placeholder when
/// something opens it. `still_dataless` is injected so the second check is
/// testable without a provider; production passes
/// [`writ_storage::notes_index::is_dataless`].
pub fn read_and_verify_with(path: &Path, still_dataless: impl Fn(&Path) -> bool) -> io::Result<()> {
    let mut file = std::fs::File::open(path)?;
    let mut chunk = vec![0u8; READ_CHUNK_BYTES];
    loop {
        match file.read(&mut chunk)? {
            0 => break,
            _ => continue,
        }
    }
    drop(file);

    if still_dataless(path) {
        return Err(io::Error::other(
            "the file is still not on this machine after the download",
        ));
    }
    Ok(())
}

/// The event state a finished download reports.
fn state_of(outcome: &DownloadOutcome) -> NoteDownloadState {
    match outcome {
        DownloadOutcome::Done => NoteDownloadState::Done,
        DownloadOutcome::Cancelled => NoteDownloadState::Cancelled,
        DownloadOutcome::Failed(_) => NoteDownloadState::Failed,
        DownloadOutcome::TimedOut => NoteDownloadState::TimedOut,
    }
}

/// The message carried alongside the state, when the provider gave one.
fn message_of(outcome: &DownloadOutcome) -> Option<String> {
    match outcome {
        DownloadOutcome::Failed(message) => Some(message.clone()),
        _ => None,
    }
}

fn emit_download(app: &AppHandle, path: &str, state: NoteDownloadState, message: Option<String>) {
    let event = WritFrontendEvent::NoteDownload {
        path: path.to_string(),
        state,
        message,
    };
    if let Err(e) = emit_event(app, event) {
        tracing::warn!(error = %e, "failed to emit note-download event");
    }
}

/// Asks the sync provider for a note's bytes, off the IPC thread.
///
/// Returns as soon as the download has started; the outcome arrives as a
/// `writ://note-download` event. A second call for a path already downloading
/// joins that download rather than starting another.
#[tauri::command]
pub fn materialise_note(
    app: AppHandle,
    state: State<'_, AppState>,
    downloads: State<'_, MaterialiseState>,
    path: String,
) -> Result<(), String> {
    let canonical = crate::commands::file::authorize_download(&state, &path)?;
    materialise_note_inner(app, &downloads, canonical)
}

/// Claims `canonical` for a new download and hands its cancel flag to `spawn`,
/// or leaves the download already running for that path alone.
///
/// Separate from [`materialise_note_inner`] so the join is testable without an
/// `AppHandle`: a second request for a path in flight must never reach `spawn`.
fn start_or_join(
    downloads: &MaterialiseState,
    canonical: &str,
    spawn: impl FnOnce(Arc<AtomicBool>),
) {
    match downloads.claim(canonical) {
        Claim::Started(cancel) => spawn(cancel),
        // Already downloading: the caller waits on the event the running
        // download will emit, and no second thread is spawned.
        Claim::Joined => (),
    }
}

/// [`materialise_note`] with the path already authorized.
pub(crate) fn materialise_note_inner(
    app: AppHandle,
    downloads: &MaterialiseState,
    canonical: String,
) -> Result<(), String> {
    start_or_join(downloads, &canonical, |cancel| {
        spawn_download(app, canonical.clone(), cancel)
    });
    Ok(())
}

/// Runs one download to its outcome on a thread of its own, releasing the path
/// and reporting to the frontend whichever way it ends.
/// Settles the open authorization a not-downloaded answer minted, once the
/// download has ended.
///
/// The token lives as long as the note's pending tab does. `Done` leaves it
/// for the open that follows the bytes landing. A download that failed or ran
/// out of time leaves its tab on screen, and opening the note again is the
/// person's next move, so the token stays for that second attempt. Only a
/// cancel ends the tab as well as the wait, and that is the one ending that
/// gives the authorization back here. Nothing in this function can mint one.
pub(crate) fn settle_authorization(
    authorized: &AuthorizedPaths,
    canonical: &str,
    outcome: &DownloadOutcome,
) {
    if matches!(outcome, DownloadOutcome::Cancelled) {
        authorized.discard_pending_open(canonical);
    }
}

fn spawn_download(app: AppHandle, canonical: String, cancel: Arc<AtomicBool>) {
    emit_download(&app, &canonical, NoteDownloadState::Started, None);

    let waiter_app = app.clone();
    let waiter_path = canonical.clone();
    std::thread::spawn(move || {
        let path = PathBuf::from(&waiter_path);
        materialise_with(
            path,
            cancel,
            DOWNLOAD_TIMEOUT,
            |p| read_and_verify_with(p, writ_storage::notes_index::is_dataless),
            |outcome| {
                let downloads = waiter_app.state::<MaterialiseState>();
                downloads.release(&waiter_path);
                let state = waiter_app.state::<AppState>();
                settle_authorization(&state.authorized_paths, &waiter_path, &outcome);
                emit_download(
                    &waiter_app,
                    &waiter_path,
                    state_of(&outcome),
                    message_of(&outcome),
                );
            },
        );
    });
}

/// Stops waiting for a note's bytes.
///
/// The provider keeps fetching — nothing here can call that off — but Writ
/// drops the result and opens nothing.
///
/// Hands back the open authorization the not-downloaded answer minted, so this
/// is also how the frontend gives the token up when the person dismisses a
/// download that ended without the note: closing the pane or its tab.
#[tauri::command]
pub fn cancel_materialise_note(
    state: State<'_, AppState>,
    downloads: State<'_, MaterialiseState>,
    path: String,
) -> Result<(), String> {
    let canonical = crate::commands::file::authorize_download(&state, &path)?;
    downloads.cancel(&canonical);
    state.authorized_paths.discard_pending_open(&canonical);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc::sync_channel;

    fn outcomes() -> (
        Arc<Mutex<Vec<DownloadOutcome>>>,
        impl FnMut(DownloadOutcome),
    ) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        (seen, move |outcome| {
            sink.lock().expect("outcome sink").push(outcome);
        })
    }

    #[test]
    fn a_read_that_finishes_reports_done() {
        let (seen, report) = outcomes();
        materialise_with(
            PathBuf::from("/notes/a.md"),
            Arc::new(AtomicBool::new(false)),
            Duration::from_secs(5),
            |_| Ok(()),
            report,
        );
        assert_eq!(*seen.lock().unwrap(), vec![DownloadOutcome::Done]);
    }

    #[test]
    fn a_read_that_fails_reports_the_reason() {
        let (seen, report) = outcomes();
        materialise_with(
            PathBuf::from("/notes/a.md"),
            Arc::new(AtomicBool::new(false)),
            Duration::from_secs(5),
            |_| Err(io::Error::other("provider is signed out")),
            report,
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        let DownloadOutcome::Failed(message) = &seen[0] else {
            panic!("expected a failure, got {:?}", seen[0]);
        };
        assert!(message.contains("provider is signed out"), "{message}");
    }

    /// How long a test waits for a wait that should have ended by itself. Long
    /// enough for a loaded machine, short enough that a wait which never ends
    /// is a red test rather than a hung job.
    const TEST_PATIENCE: Duration = Duration::from_secs(5);

    /// Runs `materialise_with` off the test thread and hands back the outcome
    /// channel, so a wait that never ends fails on `recv_timeout` instead of
    /// blocking the suite.
    fn wait_off_thread<R>(
        cancel: Arc<AtomicBool>,
        timeout: Duration,
        read: R,
    ) -> mpsc::Receiver<DownloadOutcome>
    where
        R: FnOnce(&Path) -> io::Result<()> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            materialise_with(
                PathBuf::from("/notes/a.md"),
                cancel,
                timeout,
                read,
                move |outcome| {
                    let _ = tx.send(outcome);
                },
            );
        });
        rx
    }

    #[test]
    fn a_cancelled_wait_reports_cancelled_and_drops_the_read() {
        let (release_tx, release_rx) = sync_channel::<()>(0);
        let cancel = Arc::new(AtomicBool::new(false));

        let flag = Arc::clone(&cancel);
        std::thread::spawn(move || {
            // The read is still blocked when the flag goes up.
            std::thread::sleep(Duration::from_millis(10));
            flag.store(true, Ordering::SeqCst);
        });

        let outcomes = wait_off_thread(cancel, Duration::from_secs(30), move |_| {
            let _ = release_rx.recv();
            Ok(())
        });

        assert_eq!(
            outcomes
                .recv_timeout(TEST_PATIENCE)
                .expect("the wait answers the cancel"),
            DownloadOutcome::Cancelled
        );
        // The read lands after the wait gave up; its result goes nowhere, and
        // releasing it lets the detached thread end with the test.
        let _ = release_tx.send(());
        assert!(
            outcomes.recv_timeout(Duration::from_millis(200)).is_err(),
            "the abandoned read reports nothing of its own"
        );
    }

    #[test]
    fn a_cancel_that_lands_before_the_outcome_is_never_reported_as_done() {
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let (seen, report) = outcomes();

        materialise_with(
            PathBuf::from("/notes/a.md"),
            cancel,
            Duration::from_secs(30),
            move |_| {
                // The bytes arrived, but the person had already called it off.
                // Raised before the result is sent, so the wait sees the flag
                // whichever side of the poll window the result lands on.
                flag.store(true, Ordering::SeqCst);
                Ok(())
            },
            report,
        );

        assert_eq!(*seen.lock().unwrap(), vec![DownloadOutcome::Cancelled]);
    }

    #[test]
    fn a_cancel_beats_a_read_that_failed_at_the_same_moment() {
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let (seen, report) = outcomes();

        materialise_with(
            PathBuf::from("/notes/a.md"),
            cancel,
            Duration::from_secs(30),
            move |_| {
                flag.store(true, Ordering::SeqCst);
                Err(io::Error::other("provider is signed out"))
            },
            report,
        );

        assert_eq!(*seen.lock().unwrap(), vec![DownloadOutcome::Cancelled]);
    }

    #[test]
    fn a_read_that_outlasts_the_timeout_reports_timed_out() {
        let (release_tx, release_rx) = sync_channel::<()>(0);

        let outcomes = wait_off_thread(
            Arc::new(AtomicBool::new(false)),
            Duration::from_millis(40),
            move |_| {
                let _ = release_rx.recv();
                Ok(())
            },
        );

        assert_eq!(
            outcomes
                .recv_timeout(TEST_PATIENCE)
                .expect("the wait ends on the clock"),
            DownloadOutcome::TimedOut
        );
        let _ = release_tx.send(());
    }

    #[test]
    fn reading_a_real_file_verifies_the_bytes_arrived() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "# here\n").unwrap();

        assert!(read_and_verify_with(&file, |_| false).is_ok());
    }

    #[test]
    fn a_file_still_dataless_after_the_read_fails() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "# here\n").unwrap();

        let err = read_and_verify_with(&file, |_| true).expect_err("still a placeholder");
        assert!(err.to_string().contains("still not on this machine"));
    }

    #[test]
    fn a_missing_file_fails_rather_than_hanging() {
        let dir = tempfile::TempDir::new().unwrap();
        let err =
            read_and_verify_with(&dir.path().join("gone.md"), |_| false).expect_err("no such file");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn one_path_is_claimed_once_and_released_on_the_outcome() {
        let state = MaterialiseState::default();
        let Claim::Started(cancel) = state.claim("/notes/a.md") else {
            panic!("the first claim owns the download");
        };
        assert!(state.is_in_flight("/notes/a.md"));
        assert!(matches!(state.claim("/notes/a.md"), Claim::Joined));

        state.cancel("/notes/a.md");
        assert!(cancel.load(Ordering::SeqCst));

        state.release("/notes/a.md");
        assert!(!state.is_in_flight("/notes/a.md"));
        // Released, so the path can be asked for again.
        assert!(matches!(state.claim("/notes/a.md"), Claim::Started(_)));
    }

    #[test]
    fn a_second_request_for_a_path_in_flight_starts_no_second_download() {
        let state = MaterialiseState::default();
        let spawns = Arc::new(AtomicUsize::new(0));

        let counted = |flag: Arc<AtomicBool>| {
            let _ = flag;
            spawns.fetch_add(1, Ordering::SeqCst);
        };
        start_or_join(&state, "/notes/a.md", counted);
        start_or_join(&state, "/notes/a.md", counted);
        assert_eq!(spawns.load(Ordering::SeqCst), 1);

        // A different note is its own download.
        start_or_join(&state, "/notes/b.md", counted);
        assert_eq!(spawns.load(Ordering::SeqCst), 2);

        // Once the first download has ended, the note can be asked for again.
        state.release("/notes/a.md");
        start_or_join(&state, "/notes/a.md", counted);
        assert_eq!(spawns.load(Ordering::SeqCst), 3);
    }

    // The token a not-downloaded answer minted, alongside one for another note
    // that has nothing to do with this download.
    fn authorized_with_two_tokens() -> (AuthorizedPaths, &'static str, &'static str) {
        let authorized = AuthorizedPaths::new();
        let downloading = "/notes/away.md";
        let other = "/notes/elsewhere.md";
        authorized.record_for_open(downloading.to_string());
        authorized.record_for_open(other.to_string());
        (authorized, downloading, other)
    }

    #[test]
    fn a_cancel_hands_the_open_token_back() {
        let (authorized, downloading, other) = authorized_with_two_tokens();
        settle_authorization(&authorized, downloading, &DownloadOutcome::Cancelled);
        assert!(!authorized.is_pending_open(downloading));
        assert!(
            authorized.is_pending_open(other),
            "the cancel took another note's token with it"
        );
        assert_eq!(authorized.pending_open_len(), 1);
    }

    #[test]
    fn an_ending_that_leaves_the_tab_up_keeps_the_token_for_a_second_attempt() {
        for outcome in [
            DownloadOutcome::Failed("no space".into()),
            DownloadOutcome::TimedOut,
        ] {
            let (authorized, downloading, other) = authorized_with_two_tokens();
            settle_authorization(&authorized, downloading, &outcome);
            assert!(
                authorized.is_pending_open(downloading),
                "{outcome:?} took the token the retry needs"
            );
            assert!(authorized.is_pending_open(other), "{outcome:?}");
            assert_eq!(authorized.pending_open_len(), 2, "{outcome:?}");
        }
    }

    #[test]
    fn a_download_that_arrives_leaves_the_token_for_the_open_to_spend() {
        let (authorized, downloading, other) = authorized_with_two_tokens();
        settle_authorization(&authorized, downloading, &DownloadOutcome::Done);
        assert!(authorized.is_pending_open(downloading));
        assert!(authorized.consume_for_open(downloading));
        // One open, and only one: the token is gone the moment it is spent.
        assert!(!authorized.consume_for_open(downloading));
        assert!(authorized.is_pending_open(other));
    }

    #[test]
    fn no_ending_can_authorize_a_path_of_its_own() {
        // A read detached by a cancel lands late with whatever it found.
        // Settling its outcome can give an authorization back and can leave
        // one alone; it has no way to record or extend one.
        let authorized = AuthorizedPaths::new();
        for outcome in [
            DownloadOutcome::Done,
            DownloadOutcome::Cancelled,
            DownloadOutcome::Failed("late".into()),
            DownloadOutcome::TimedOut,
        ] {
            settle_authorization(&authorized, "/notes/away.md", &outcome);
            assert!(!authorized.is_pending_open("/notes/away.md"), "{outcome:?}");
            assert_eq!(authorized.pending_open_len(), 0, "{outcome:?}");
        }
    }

    #[test]
    fn cancelling_a_path_nothing_is_downloading_does_nothing() {
        let state = MaterialiseState::default();
        state.cancel("/notes/never.md");
        assert!(!state.is_in_flight("/notes/never.md"));
    }
}
