//! The OpenRouter connect flow: a one-shot loopback receiver and a key
//! exchange (ADR-040 section 6).
//!
//! The pure half lives in [`writ_core::ai::pkce`]. This module is the
//! mechanism: it reads 48 bytes of entropy, binds a listener on `127.0.0.1`
//! with an ephemeral port, sends the user's browser to the authorization page
//! through the opener, accepts exactly one request, and exchanges the code for
//! a key that is stored the way a typed key is stored.
//!
//! Privacy and lifetime invariants enforced here:
//! - Writ listens on no port at rest. The listener exists for the duration of
//!   one flow the user started, accepts one request, and is dropped before that
//!   request is even read, so nothing can arrive behind it (ADR-031 rule 2.4 as
//!   amended by ADR-040 section 10).
//! - The exchange is a request to the configured host, so it waits for Allow
//!   like every other one. The host is resolved through the same path
//!   `ai_consent_host` records under, never spelled out here, so the string
//!   tested for membership is the string Allow wrote.
//! - The verifier, the code and the returned key never reach a log line or an
//!   error string. Only an outcome word and an HTTP status are recorded, and
//!   the four failures a user can see say nothing about which one happened
//!   beyond its own sentence.
//! - A second Connect cancels the first, and a flow that finishes clears the
//!   registration only when it still owns it, so a slow first flow cannot
//!   silently unregister a second.

use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;
use writ_core::ai::pkce;
use writ_core::ai::providers;
use writ_core::polish;

use super::ai::{self, AiKeyState, AiState};
use crate::poison::recover_poison;
use crate::state::AppState;

/// The one provider with a Connect button, and the keychain account the key
/// lands under.
const PROVIDER: &str = "openrouter";

/// Where the code is exchanged for a key.
const KEYS_URL: &str = "https://openrouter.ai/api/v1/auth/keys";

/// How long a flow may wait for the browser before it gives up.
const FLOW_BUDGET: Duration = Duration::from_secs(300);

/// How often the receiver looks at the cancel flag and the deadline between
/// two non-blocking `accept` attempts.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// How long the accepted socket may take to deliver its request line.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// The most of one request that is read. A callback is a few hundred bytes;
/// anything larger is not one, and reading it would be reading whatever a
/// program on this machine chose to send.
const MAX_REQUEST: usize = 8 * 1024;

/// The exchange's own budget, shorter than the rewrite client's.
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(30);

/// What the browser is left showing after a callback that checked out.
const DONE_PAGE: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Writ</title></head><body><p>You can close this window and go back to Writ.</p></body></html>";

/// What it is left showing after one that did not. Says nothing about which
/// rule rejected it.
const REJECTED_PAGE: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Writ</title></head><body><p>This link is not valid.</p></body></html>";

// --- Flow registration -----------------------------------------------------

/// The flow that is running, if one is.
struct ActiveFlow {
    /// Set to stop the receiver. Its identity is also what says whether a
    /// finishing flow is still the registered one.
    cancel: Arc<AtomicBool>,
    /// Where the receiver is listening, so a cancel can knock on it.
    addr: SocketAddr,
}

/// Session state for the connect flow, managed alongside [`AiState`] so the
/// key store is not widened by something that is not a key.
#[derive(Default)]
pub struct ConnectState {
    active: Mutex<Option<ActiveFlow>>,
}

/// Registers `cancel`/`addr` as the running flow and stops whatever was
/// registered before it.
///
/// The old flag is set under the same lock that installs the new flow, so a
/// first flow can never come back to life between the two. The knock that
/// unblocks it happens after the lock is dropped.
fn register_flow(connect: &ConnectState, cancel: &Arc<AtomicBool>, addr: SocketAddr) {
    let previous = {
        let mut guard =
            recover_poison(connect.active.lock(), "commands::ai_connect::register_flow");
        let previous = guard.take();
        if let Some(flow) = previous.as_ref() {
            flow.cancel.store(true, Ordering::SeqCst);
        }
        *guard = Some(ActiveFlow {
            cancel: Arc::clone(cancel),
            addr,
        });
        previous
    };
    if let Some(flow) = previous {
        knock(flow.addr);
    }
}

/// Stops the running flow, if there is one.
fn stop_flow(connect: &ConnectState) {
    let flow = {
        let mut guard = recover_poison(connect.active.lock(), "commands::ai_connect::stop_flow");
        let flow = guard.take();
        if let Some(flow) = flow.as_ref() {
            flow.cancel.store(true, Ordering::SeqCst);
        }
        flow
    };
    if let Some(flow) = flow {
        knock(flow.addr);
    }
}

/// Clears the registration only when `cancel` is still the flag it holds.
///
/// A flow that took the long way round must not unregister the flow that
/// replaced it, or the next cancel would find nothing to stop.
fn clear_flow(connect: &ConnectState, cancel: &Arc<AtomicBool>) {
    let mut guard = recover_poison(connect.active.lock(), "commands::ai_connect::clear_flow");
    let owned = guard
        .as_ref()
        .is_some_and(|flow| Arc::ptr_eq(&flow.cancel, cancel));
    if owned {
        *guard = None;
    }
}

/// Opens and closes one connection to `addr`, so a receiver sitting between two
/// looks at its cancel flag has something to return from.
fn knock(addr: SocketAddr) {
    if let Ok(stream) = TcpStream::connect_timeout(&addr, POLL_INTERVAL * 4) {
        let _ = stream.shutdown(Shutdown::Both);
    }
}

// --- Failures --------------------------------------------------------------

/// How a flow ended, when it did not end with a key.
///
/// Each variant has one sentence, and the sentence is all the user is told:
/// which of the four happened is the only thing a failed Connect says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectError {
    /// The user, or a second Connect, stopped it.
    Cancelled,
    /// The browser never came back.
    TimedOut,
    /// The callback was not this flow's, or the socket could not be worked.
    ///
    /// The contract's sentence for this one used a word ADR-028 §10 retires
    /// from user-visible messages, which `user_facing_strings_tests` fails on.
    /// The wording moved; the shape did not.
    Refused,
    /// The code could not be turned into a key.
    Exchange,
}

impl ConnectError {
    /// The one sentence the user sees.
    fn message(self) -> String {
        match self {
            Self::Cancelled => "Connect was cancelled.".to_string(),
            Self::TimedOut => "OpenRouter did not answer within five minutes.".to_string(),
            Self::Refused => "OpenRouter did not accept the connection.".to_string(),
            Self::Exchange => "The key exchange failed.".to_string(),
        }
    }

    /// The word that is recorded. Nothing else about the flow is.
    fn outcome(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Refused => "refused",
            Self::Exchange => "exchange_failed",
        }
    }
}

/// Why the exchange did not produce a key. Every variant reaches the user as
/// [`ConnectError::Exchange`]; they are separate so the status can be recorded
/// and the tests can say which half failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExchangeError {
    /// The request never completed.
    Transport,
    /// The host answered something other than a 2xx.
    Status(u16),
    /// A 2xx body with no key in it.
    Malformed,
}

// --- The receiver ----------------------------------------------------------

/// Waits for the one callback this flow is expecting and answers it.
///
/// Blocking, so it runs on a blocking thread. The listener is polled rather
/// than blocked on, so the cancel flag and the deadline are looked at between
/// attempts; it is dropped the moment a connection is accepted, which is what
/// makes the receiver one-shot and leaves no port behind.
fn await_callback(
    listener: TcpListener,
    expected_state: &str,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<String, ConnectError> {
    // A socket that cannot be polled is a flow that cannot be stopped, so it
    // is not started.
    listener
        .set_nonblocking(true)
        .map_err(|_| ConnectError::Refused)?;

    let mut stream = loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(ConnectError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(ConnectError::TimedOut);
        }
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(_) => return Err(ConnectError::Refused),
        }
    };
    // One request and no port. Everything still in the backlog is reset here,
    // and a second browser window reaches nothing.
    drop(listener);

    // The accepted socket inherits the listener's non-blocking flag on the BSD
    // sockets macOS uses, and a read on it would answer at once with nothing.
    stream
        .set_nonblocking(false)
        .map_err(|_| ConnectError::Refused)?;
    // A cancel that raced the knock in is a cancel, not a bad callback.
    if cancel.load(Ordering::SeqCst) {
        return Err(ConnectError::Cancelled);
    }
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));

    let line = read_request_line(&mut stream);
    match pkce::parse_callback(&line) {
        Ok(callback) if pkce::check_state(expected_state, &callback.state) => {
            respond(&mut stream, "200 OK", DONE_PAGE);
            Ok(callback.code)
        }
        _ => {
            respond(&mut stream, "400 Bad Request", REJECTED_PAGE);
            Err(ConnectError::Refused)
        }
    }
}

/// Reads up to [`MAX_REQUEST`] bytes and hands back the first line.
///
/// Stops at the first newline: the request line is the whole message as far as
/// this receiver is concerned, and the headers behind it are never looked at.
fn read_request_line(stream: &mut TcpStream) -> String {
    let mut buffer = vec![0u8; MAX_REQUEST];
    let mut filled = 0usize;
    while filled < buffer.len() {
        match stream.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => {
                filled += read;
                if buffer[..filled].contains(&b'\n') {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let text = String::from_utf8_lossy(&buffer[..filled]);
    text.lines().next().unwrap_or_default().to_string()
}

/// Writes one static page and closes the socket. A write that fails changes
/// nothing: the flow's answer is already decided.
fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
}

// --- The exchange ----------------------------------------------------------

/// Turns a code and its verifier into a key.
///
/// The network is the `post` argument, so the parse is tested against recorded
/// answers. In the app `post` ignores what it is handed and returns an already
/// awaited body: the request is async and this seam is not, which is the one
/// place the shape bends to the runtime.
fn exchange_key<F>(code: &str, verifier: &str, post: F) -> Result<String, ExchangeError>
where
    F: FnOnce(&str, &str) -> Result<String, ExchangeError>,
{
    key_from_body(&post(code, verifier)?)
}

/// Reads `{ "key": "…" }`. Anything else is malformed, including a body that
/// parses but carries an empty key.
fn key_from_body(body: &str) -> Result<String, ExchangeError> {
    let parsed: serde_json::Value =
        serde_json::from_str(body).map_err(|_| ExchangeError::Malformed)?;
    match parsed.get("key").and_then(serde_json::Value::as_str) {
        Some(key) if !key.is_empty() => Ok(key.to_string()),
        _ => Err(ExchangeError::Malformed),
    }
}

/// Posts the code and the verifier, and hands back the body.
///
/// Only the status is recorded. The body is not read into a log line, because
/// a failing provider is as likely to echo the code back as anything else.
async fn post_keys(
    client: &reqwest::Client,
    code: &str,
    verifier: &str,
) -> Result<String, ExchangeError> {
    let response = client
        .post(KEYS_URL)
        .timeout(EXCHANGE_TIMEOUT)
        .json(&serde_json::json!({
            "code": code,
            "code_verifier": verifier,
            "code_challenge_method": "S256",
        }))
        .send()
        .await
        .map_err(|_| ExchangeError::Transport)?;

    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        tracing::warn!(status, "the connect exchange was answered with a failure");
        return Err(ExchangeError::Status(status));
    }
    response.text().await.map_err(|_| ExchangeError::Transport)
}

// --- Commands --------------------------------------------------------------

/// The host the connect flow reaches, resolved the way consent records it.
///
/// Derived from the provider table through [`polish::resolve_endpoint`], so the
/// string looked for in `consented_hosts` is byte-identical to the one
/// `ai_consent_host` wrote. A literal here could drift out of agreement with
/// the guard and refuse a connection the user already allowed.
fn connect_host() -> Result<String, String> {
    let row = providers::provider(PROVIDER).ok_or_else(|| ConnectError::Refused.message())?;
    let target =
        polish::resolve_endpoint(row.base_url).map_err(|_| ConnectError::Refused.message())?;
    Ok(target.host)
}

/// Reads the entropy one flow needs: 32 bytes for the verifier, 16 for the
/// state.
fn flow_secrets() -> Result<(pkce::Verifier, String), ConnectError> {
    let mut verifier_bytes = [0u8; 32];
    let mut state_bytes = [0u8; 16];
    // No entropy is no credential, and a predictable verifier would be worse
    // than no flow at all.
    getrandom::fill(&mut verifier_bytes).map_err(|_| ConnectError::Exchange)?;
    getrandom::fill(&mut state_bytes).map_err(|_| ConnectError::Exchange)?;
    Ok((
        pkce::Verifier::from_bytes(&verifier_bytes),
        pkce::state_from_bytes(&state_bytes),
    ))
}

/// IPC: connect to OpenRouter and store the key it issues.
///
/// Resolves with the key state once the key is stored under `openrouter`.
/// Rejects with one of four sentences and nothing more.
#[tauri::command]
pub async fn ai_openrouter_connect(app: AppHandle) -> Result<AiKeyState, String> {
    let host = connect_host()?;
    {
        let state = app.state::<AppState>();
        let guard = recover_poison(
            state.config.lock(),
            "commands::ai_connect::ai_openrouter_connect",
        );
        if !ai::is_consented(&guard.ai, &host) {
            return Err("OpenRouter is not allowed yet.".to_string());
        }
    }

    let client = ai::build_client()?;
    let (verifier, expected_state) = flow_secrets().map_err(ConnectError::message)?;

    let listener =
        TcpListener::bind(("127.0.0.1", 0)).map_err(|_| ConnectError::Refused.message())?;
    let addr = listener
        .local_addr()
        .map_err(|_| ConnectError::Refused.message())?;

    let cancel = Arc::new(AtomicBool::new(false));
    register_flow(&app.state::<ConnectState>(), &cancel, addr);

    let url = pkce::auth_url(
        &format!("http://{addr}/callback"),
        &pkce::challenge_s256(verifier.as_str()),
        &expected_state,
    );
    if app.opener().open_url(url, None::<&str>).is_err() {
        // The opener's own error can echo the URL it was handed, and that URL
        // carries this flow's values; the outcome word is the whole record.
        tracing::warn!(
            outcome = ConnectError::Refused.outcome(),
            "the connect page could not be opened"
        );
        clear_flow(&app.state::<ConnectState>(), &cancel);
        return Err(ConnectError::Refused.message());
    }

    let deadline = Instant::now() + FLOW_BUDGET;
    let waiting_state = expected_state.clone();
    let waiting_cancel = Arc::clone(&cancel);
    let received = tauri::async_runtime::spawn_blocking(move || {
        await_callback(listener, &waiting_state, waiting_cancel, deadline)
    })
    .await;

    let code = match received.unwrap_or(Err(ConnectError::Refused)) {
        Ok(code) => code,
        Err(failure) => {
            clear_flow(&app.state::<ConnectState>(), &cancel);
            tracing::info!(outcome = failure.outcome(), "the connect flow ended");
            return Err(failure.message());
        }
    };

    let fetched = post_keys(&client, &code, verifier.as_str()).await;
    let key = match exchange_key(&code, verifier.as_str(), move |_, _| fetched) {
        Ok(key) => key,
        Err(_) => {
            clear_flow(&app.state::<ConnectState>(), &cancel);
            tracing::info!(
                outcome = ConnectError::Exchange.outcome(),
                "the connect flow ended"
            );
            return Err(ConnectError::Exchange.message());
        }
    };

    clear_flow(&app.state::<ConnectState>(), &cancel);
    // The same path a typed key takes: the store, the cache invalidation and
    // the memory fallback are not written twice.
    let stored = ai::ai_set_api_key(app.state::<AiState>(), PROVIDER.to_string(), key)?;
    tracing::info!(outcome = "connected", "the connect flow ended");
    Ok(stored)
}

/// IPC: stop the running connect flow. Does nothing when none is running.
#[tauri::command]
pub fn ai_openrouter_cancel(app: AppHandle) {
    stop_flow(&app.state::<ConnectState>());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A listener on an ephemeral loopback port, and where it is.
    fn bound() -> (TcpListener, SocketAddr) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a loopback port");
        let addr = listener.local_addr().expect("the bound address");
        (listener, addr)
    }

    /// Runs the receiver on its own thread, the way the command runs it on a
    /// blocking one.
    fn receive(
        listener: TcpListener,
        expected_state: &str,
        cancel: Arc<AtomicBool>,
        deadline: Instant,
    ) -> std::thread::JoinHandle<Result<String, ConnectError>> {
        let expected_state = expected_state.to_string();
        std::thread::spawn(move || await_callback(listener, &expected_state, cancel, deadline))
    }

    fn request(code: &str, state: &str) -> String {
        format!("GET /callback?code={code}&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
    }

    fn read_answer(stream: &mut TcpStream) -> String {
        let mut answer = String::new();
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = stream.read_to_string(&mut answer);
        answer
    }

    fn soon() -> Instant {
        Instant::now() + Duration::from_secs(10)
    }

    #[test]
    fn a_good_callback_answers_the_browser_and_leaves_no_port() {
        let (listener, addr) = bound();
        let cancel = Arc::new(AtomicBool::new(false));
        let waiting = receive(listener, "st4te", Arc::clone(&cancel), soon());

        let mut browser = TcpStream::connect(addr).expect("the callback connects");
        browser
            .write_all(request("c0de", "st4te").as_bytes())
            .expect("the callback is sent");
        let answer = read_answer(&mut browser);

        assert_eq!(
            waiting.join().expect("the receiver finishes"),
            Ok("c0de".to_string())
        );
        assert!(answer.starts_with("HTTP/1.1 200 OK"), "answer: {answer}");
        assert!(answer.contains("You can close this window and go back to Writ."));
        assert!(
            TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_err(),
            "the port was still open after the flow"
        );
    }

    #[test]
    fn a_mismatched_state_is_refused_and_the_browser_is_told() {
        let (listener, addr) = bound();
        let cancel = Arc::new(AtomicBool::new(false));
        let waiting = receive(listener, "st4te", Arc::clone(&cancel), soon());

        let mut browser = TcpStream::connect(addr).expect("the callback connects");
        browser
            .write_all(request("c0de", "another").as_bytes())
            .expect("the callback is sent");
        let answer = read_answer(&mut browser);

        assert_eq!(
            waiting.join().expect("the receiver finishes"),
            Err(ConnectError::Refused)
        );
        assert!(answer.starts_with("HTTP/1.1 400"), "answer: {answer}");
        assert!(answer.contains("This link is not valid."));
        assert!(!answer.contains("c0de"), "the answer echoed the code");
    }

    #[test]
    fn a_cancel_from_another_thread_ends_the_wait() {
        let (listener, addr) = bound();
        let cancel = Arc::new(AtomicBool::new(false));
        let waiting = receive(listener, "st4te", Arc::clone(&cancel), soon());

        let started = Instant::now();
        cancel.store(true, Ordering::SeqCst);
        knock(addr);

        assert_eq!(
            waiting.join().expect("the receiver finishes"),
            Err(ConnectError::Cancelled)
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the cancel took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_deadline_already_past_times_out() {
        let (listener, _addr) = bound();
        let past = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .unwrap_or_else(Instant::now);
        let result = await_callback(listener, "st4te", Arc::new(AtomicBool::new(false)), past);
        assert_eq!(result, Err(ConnectError::TimedOut));
    }

    #[test]
    fn only_the_first_request_is_accepted() {
        let (listener, addr) = bound();
        let mut first = TcpStream::connect(addr).expect("the first connects");
        let mut second = TcpStream::connect(addr).expect("the second joins the backlog");
        first
            .write_all(request("one", "st4te").as_bytes())
            .expect("the first callback is sent");

        let received = await_callback(listener, "st4te", Arc::new(AtomicBool::new(false)), soon());
        assert_eq!(received, Ok("one".to_string()));

        let _ = second.write_all(request("two", "st4te").as_bytes());
        let answer = read_answer(&mut second);
        assert!(
            answer.is_empty(),
            "the second request was answered: {answer}"
        );
    }

    #[test]
    fn a_recorded_answer_yields_its_key() {
        let body = r#"{"key":"sk-or-v1-recorded"}"#.to_string();
        let key = exchange_key("c0de", "verifier", |code, verifier| {
            assert_eq!(code, "c0de");
            assert_eq!(verifier, "verifier");
            Ok(body)
        });
        assert_eq!(key, Ok("sk-or-v1-recorded".to_string()));
    }

    #[test]
    fn a_malformed_answer_is_not_a_key() {
        for body in [
            "not json",
            "{}",
            r#"{"key":""}"#,
            r#"{"key":42}"#,
            r#"{"error":"nope"}"#,
        ] {
            assert_eq!(
                exchange_key("c0de", "verifier", |_, _| Ok(body.to_string())),
                Err(ExchangeError::Malformed),
                "body: {body}"
            );
        }
    }

    #[test]
    fn a_failed_request_never_reaches_the_parse() {
        assert_eq!(
            exchange_key("c0de", "verifier", |_, _| Err(ExchangeError::Status(403))),
            Err(ExchangeError::Status(403))
        );
        assert_eq!(
            exchange_key("c0de", "verifier", |_, _| Err(ExchangeError::Transport)),
            Err(ExchangeError::Transport)
        );
    }

    #[test]
    fn every_failure_says_one_sentence_and_no_more() {
        let sentences: Vec<String> = [
            ConnectError::Cancelled,
            ConnectError::TimedOut,
            ConnectError::Refused,
            ConnectError::Exchange,
        ]
        .into_iter()
        .map(ConnectError::message)
        .collect();
        assert_eq!(
            sentences,
            vec![
                "Connect was cancelled.",
                "OpenRouter did not answer within five minutes.",
                "OpenRouter did not accept the connection.",
                "The key exchange failed.",
            ]
        );
    }

    #[test]
    fn the_consent_host_is_the_one_allow_records() {
        let host = connect_host().expect("the table row resolves");
        assert_eq!(host, "openrouter.ai");
        let row = providers::provider(PROVIDER).expect("the row exists");
        assert!(row.supports_connect);
        assert_eq!(
            host,
            polish::resolve_endpoint(row.base_url)
                .expect("resolves")
                .host
        );
    }

    #[test]
    fn a_flow_that_finished_late_does_not_unregister_the_one_that_replaced_it() {
        let connect = ConnectState::default();
        let (_first, first_addr) = bound();
        let (_second, second_addr) = bound();

        let first = Arc::new(AtomicBool::new(false));
        register_flow(&connect, &first, first_addr);
        let second = Arc::new(AtomicBool::new(false));
        register_flow(&connect, &second, second_addr);

        // Registering the second stopped the first.
        assert!(first.load(Ordering::SeqCst));
        assert!(!second.load(Ordering::SeqCst));

        clear_flow(&connect, &first);
        stop_flow(&connect);
        assert!(
            second.load(Ordering::SeqCst),
            "the late first flow unregistered the second"
        );
    }

    #[test]
    fn stopping_when_nothing_runs_changes_nothing() {
        let connect = ConnectState::default();
        stop_flow(&connect);
        let cancel = Arc::new(AtomicBool::new(false));
        clear_flow(&connect, &cancel);
        assert!(!cancel.load(Ordering::SeqCst));
    }

    #[test]
    fn every_command_here_is_in_the_invoke_handler() {
        // A command that is not registered cannot be called however well it
        // behaves, and the Connect button calls both.
        const LIB_RS: &str = include_str!("../lib.rs");
        for command in [
            "commands::ai_connect::ai_openrouter_connect",
            "commands::ai_connect::ai_openrouter_cancel",
        ] {
            assert!(LIB_RS.contains(command), "{command} is not registered");
        }
        assert!(
            LIB_RS.contains("commands::ai_connect::ConnectState"),
            "the connect state is not managed"
        );
    }
}
