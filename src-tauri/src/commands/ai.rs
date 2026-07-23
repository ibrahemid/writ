//! IPC surface for opt-in text rewriting.
//!
//! The *policy* — which actions exist, what prompts they produce, and which
//! endpoints may be contacted — lives in [`writ_core::polish`]. This module is
//! the *mechanism*: it resolves the configured endpoint and key, streams an
//! OpenAI-compatible `chat/completions` response, and mirrors each SSE delta to
//! the frontend as a `writ://ai-rewrite` event.
//!
//! Privacy invariants enforced here:
//! - The endpoint guard ([`writ_core::polish::is_endpoint_allowed`]) runs on
//!   every request against the parsed host, so a hand-edited `config.toml`
//!   pointing `http` at a remote host is rejected before any bytes leave.
//! - API keys never touch `config.toml`, the database, or disk: they live in
//!   the OS keychain, or in memory for the session when the keychain is
//!   unavailable.
//! - Only lengths and status codes are logged. Prompt text, response text, and
//!   keys never reach the logs or error strings shown to the user.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use writ_core::config::AiConfig;
use writ_core::polish::{self, PolishAction, POLISH_TEMPERATURE};

use crate::events::{emit_event, WritFrontendEvent};
use crate::poison::recover_poison;
use crate::state::AppState;

/// Keychain service name under which provider keys are stored. The account is
/// the preset id, so switching presets keeps independent keys.
const KEYCHAIN_SERVICE: &str = "com.writ.ai";

/// Connect timeout: a local Ollama that is not running should fail fast.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Overall request budget for a single rewrite.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Session-scoped runtime state for rewriting, managed separately from
/// [`AppState`] so the large app initializer stays untouched.
#[derive(Default)]
pub struct AiState {
    /// In-memory keys, keyed by preset, used only when the OS keychain is
    /// unavailable or access was denied. Never persisted.
    keys: Mutex<HashMap<String, String>>,
    /// Cancel flags for in-flight streams, keyed by the frontend's request id.
    tasks: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

/// Whether a key is stored for a preset, and whether it is confined to memory
/// for this session (keychain unavailable). Surfaced so the UI can warn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AiKeyState {
    /// A key exists for this preset.
    pub is_set: bool,
    /// The key lives only in memory this session; it will be gone on restart.
    pub memory_only: bool,
}

// --- Platform keychain -----------------------------------------------------

/// Native keychain access. Returns `Ok(None)` when no credential exists;
/// `Err` when the platform store is unavailable or access was denied, which
/// steers callers to the in-memory fallback.
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod keychain {
    use super::KEYCHAIN_SERVICE;
    use keyring::{Entry, Error};

    fn entry(account: &str) -> Result<Entry, String> {
        Entry::new(KEYCHAIN_SERVICE, account).map_err(|e| e.to_string())
    }

    pub fn set(account: &str, key: &str) -> Result<(), String> {
        entry(account)?.set_password(key).map_err(|e| e.to_string())
    }

    pub fn get(account: &str) -> Result<Option<String>, String> {
        match entry(account)?.get_password() {
            Ok(k) => Ok(Some(k)),
            Err(Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn delete(account: &str) -> Result<(), String> {
        match entry(account)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// No native keychain on this platform; callers always use the in-memory
/// fallback.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod keychain {
    pub fn set(_account: &str, _key: &str) -> Result<(), String> {
        Err("no native keychain on this platform".to_string())
    }
    pub fn get(_account: &str) -> Result<Option<String>, String> {
        Err("no native keychain on this platform".to_string())
    }
    pub fn delete(_account: &str) -> Result<(), String> {
        Ok(())
    }
}

/// Pure key-state policy: keychain wins; otherwise a memory entry is
/// "set but session-only". Separated from the OS call so it is testable.
fn compute_key_state(keychain_hit: bool, memory: &HashMap<String, String>, preset: &str) -> AiKeyState {
    if keychain_hit {
        AiKeyState {
            is_set: true,
            memory_only: false,
        }
    } else if memory.contains_key(preset) {
        AiKeyState {
            is_set: true,
            memory_only: true,
        }
    } else {
        AiKeyState {
            is_set: false,
            memory_only: false,
        }
    }
}

/// Pure key resolution: keychain value, else the memory value.
fn resolve_key_from(
    keychain_value: Option<String>,
    memory: &HashMap<String, String>,
    preset: &str,
) -> Option<String> {
    keychain_value.or_else(|| memory.get(preset).cloned())
}

fn key_state(memory: &HashMap<String, String>, preset: &str) -> AiKeyState {
    let hit = matches!(keychain::get(preset), Ok(Some(_)));
    compute_key_state(hit, memory, preset)
}

fn resolve_key(memory: &HashMap<String, String>, preset: &str) -> Option<String> {
    resolve_key_from(keychain::get(preset).ok().flatten(), memory, preset)
}

/// Stores a provider key. Prefers the OS keychain; on failure holds the key in
/// memory for the session and reports that state. Never writes the key to disk
/// and never returns it.
#[tauri::command]
pub fn ai_set_api_key(
    ai: State<'_, AiState>,
    preset: String,
    key: String,
) -> Result<AiKeyState, String> {
    if key.is_empty() {
        return Err("The API key is empty.".to_string());
    }
    let mut memory = recover_poison(ai.keys.lock(), "commands::ai::ai_set_api_key");
    match keychain::set(&preset, &key) {
        Ok(()) => {
            memory.remove(&preset);
            Ok(AiKeyState {
                is_set: true,
                memory_only: false,
            })
        }
        Err(reason) => {
            // `reason` is a keychain error; it never contains the key.
            tracing::warn!(error = %reason, "keychain unavailable; holding key in memory for this session");
            memory.insert(preset, key);
            Ok(AiKeyState {
                is_set: true,
                memory_only: true,
            })
        }
    }
}

/// Removes a provider key from both the keychain and memory.
#[tauri::command]
pub fn ai_clear_api_key(ai: State<'_, AiState>, preset: String) -> Result<AiKeyState, String> {
    let mut memory = recover_poison(ai.keys.lock(), "commands::ai::ai_clear_api_key");
    memory.remove(&preset);
    if let Err(reason) = keychain::delete(&preset) {
        tracing::debug!(error = %reason, "keychain delete failed");
    }
    Ok(AiKeyState {
        is_set: false,
        memory_only: false,
    })
}

/// Reports whether a key is set for a preset, without returning it.
#[tauri::command]
pub fn ai_has_api_key(ai: State<'_, AiState>, preset: String) -> Result<AiKeyState, String> {
    let memory = recover_poison(ai.keys.lock(), "commands::ai::ai_has_api_key");
    Ok(key_state(&memory, &preset))
}

// --- Request preparation (pure, testable) ----------------------------------

/// Everything a stream needs, resolved from config and validated. Building this
/// performs every pre-flight check, so the async task only does I/O.
#[derive(Debug, Clone, PartialEq)]
struct PreparedRequest {
    endpoint: String,
    body: serde_json::Value,
    api_key: Option<String>,
    is_localhost: bool,
}

/// Validates config + inputs and resolves the request. `lookup_key` maps a
/// preset to its key (keychain or memory); it is only consulted for hosted
/// endpoints. Errors carry a plain, secret-free message for the UI.
fn prepare_request(
    cfg: &AiConfig,
    action_id: &str,
    text: &str,
    custom_instruction: Option<String>,
    lookup_key: impl FnOnce(&str) -> Option<String>,
) -> Result<PreparedRequest, String> {
    if !cfg.enabled {
        return Err("Rewriting is turned off.".to_string());
    }

    let action = PolishAction::parse(action_id, custom_instruction).map_err(|e| e.to_string())?;
    let messages = polish::build_messages(&action, text).map_err(|e| e.to_string())?;

    let url = url::Url::parse(cfg.base_url.trim())
        .map_err(|_| "The base URL is not a valid URL.".to_string())?;
    let scheme = url.scheme();
    let host = url
        .host_str()
        .ok_or_else(|| "The base URL has no host.".to_string())?;
    if !polish::is_endpoint_allowed(scheme, host) {
        return Err("This base URL is not allowed. Use https, or http for localhost.".to_string());
    }

    if cfg.model.trim().is_empty() {
        return Err("Set a model id first.".to_string());
    }

    let hosted = polish::is_hosted(host);
    let api_key = if hosted {
        if !cfg.consented_hosted {
            return Err("Confirm sending text to this provider first.".to_string());
        }
        match lookup_key(&cfg.preset) {
            Some(k) => Some(k),
            None => return Err("Add an API key for this provider first.".to_string()),
        }
    } else {
        None
    };

    let base = cfg.base_url.trim().trim_end_matches('/');
    let endpoint = format!("{base}/chat/completions");
    let body = serde_json::json!({
        "model": cfg.model.trim(),
        "messages": messages,
        "stream": true,
        "temperature": POLISH_TEMPERATURE,
    });

    Ok(PreparedRequest {
        endpoint,
        body,
        api_key,
        is_localhost: polish::is_localhost(host),
    })
}

// --- Streaming engine (pure over its callback, testable) -------------------

/// One thing that happens during a stream.
enum StreamEvent {
    Chunk(String),
    Done,
    Error(String),
}

/// One parsed SSE `data:` line.
enum SseLine {
    Chunk(String),
    Done,
    Ignore,
}

/// Parses a single already-trimmed SSE line. Non-`data:` lines, keep-alives,
/// empty deltas, and unparseable payloads are ignored.
fn parse_sse_line(line: &str) -> SseLine {
    let Some(rest) = line.strip_prefix("data:") else {
        return SseLine::Ignore;
    };
    let payload = rest.trim();
    if payload.is_empty() {
        return SseLine::Ignore;
    }
    if payload == "[DONE]" {
        return SseLine::Done;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return SseLine::Ignore;
    };
    let content = value
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("delta"))
        .and_then(|d| d.get("content"))
        .and_then(|t| t.as_str());
    match content {
        Some(s) if !s.is_empty() => SseLine::Chunk(s.to_string()),
        _ => SseLine::Ignore,
    }
}

/// Drains every complete (newline-terminated) line from `buf`, leaving any
/// trailing partial line in place. Splitting the byte buffer on `\n` is
/// UTF-8-safe because a newline never appears inside a multibyte sequence, so a
/// chunk boundary mid-character cannot corrupt a decoded line.
fn drain_complete_lines(buf: &mut Vec<u8>) -> Vec<String> {
    let mut lines = Vec::new();
    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
        let raw: Vec<u8> = buf.drain(..=pos).collect();
        lines.push(String::from_utf8_lossy(&raw).trim().to_string());
    }
    lines
}

/// Sends the request and streams the response, invoking `on_event` for each
/// delta, the terminal `Done`, or an `Error`. Stops early when `cancel` is set,
/// emitting nothing further.
async fn run_rewrite_stream(
    client: &reqwest::Client,
    prepared: &PreparedRequest,
    cancel: &AtomicBool,
    mut on_event: impl FnMut(StreamEvent),
) {
    let mut builder = client.post(&prepared.endpoint).json(&prepared.body);
    if let Some(key) = &prepared.api_key {
        builder = builder.bearer_auth(key);
    }

    let response = match builder.send().await {
        Ok(resp) => resp,
        Err(err) => {
            on_event(StreamEvent::Error(connection_error_message(
                &err,
                prepared.is_localhost,
            )));
            return;
        }
    };

    let status = response.status();
    if !status.is_success() {
        tracing::warn!(status = status.as_u16(), "rewrite request rejected");
        on_event(StreamEvent::Error(format!(
            "The model server returned status {}.",
            status.as_u16()
        )));
        return;
    }

    if cancel.load(Ordering::Relaxed) {
        return;
    }

    let mut stream = response.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(item) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let bytes = match item {
            Ok(b) => b,
            Err(err) => {
                on_event(StreamEvent::Error(sanitize_ai_error(&err.to_string())));
                return;
            }
        };
        buf.extend_from_slice(&bytes);
        for line in drain_complete_lines(&mut buf) {
            match parse_sse_line(&line) {
                SseLine::Chunk(content) => on_event(StreamEvent::Chunk(content)),
                SseLine::Done => {
                    on_event(StreamEvent::Done);
                    return;
                }
                SseLine::Ignore => {}
            }
        }
    }

    if cancel.load(Ordering::Relaxed) {
        return;
    }
    on_event(StreamEvent::Done);
}

/// Turns a connection failure into a plain message, hinting at a stopped local
/// server when the target was loopback.
fn connection_error_message(err: &reqwest::Error, is_localhost: bool) -> String {
    if err.is_connect() && is_localhost {
        return "Could not reach the local model server. Is Ollama running?".to_string();
    }
    sanitize_ai_error(&err.to_string())
}

/// Redacts any URL from an error string so a configured endpoint (which may
/// carry a token in a query) never reaches logs or the UI. Mirrors the update
/// path's redaction; falls back to a generic message when nothing is left.
fn sanitize_ai_error(raw: &str) -> String {
    const REDACTED: &str = "<redacted-url>";
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while !rest.is_empty() {
        if rest.starts_with("http://") || rest.starts_with("https://") {
            out.push_str(REDACTED);
            let end = rest
                .find(|c: char| {
                    c.is_whitespace() || matches!(c, '(' | ')' | '"' | '\'' | '<' | '>' | ',')
                })
                .unwrap_or(rest.len());
            rest = &rest[end..];
        } else {
            let mut chars = rest.chars();
            let c = chars.next().expect("rest is non-empty");
            out.push(c);
            rest = chars.as_str();
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        "Rewrite failed.".to_string()
    } else {
        collapsed
    }
}

// --- Commands --------------------------------------------------------------

fn emit_ai(app: &AppHandle, request_id: &str, kind: &str, text: Option<String>) {
    if let Err(e) = emit_event(
        app,
        WritFrontendEvent::AiRewrite {
            request_id: request_id.to_string(),
            kind: kind.to_string(),
            text,
        },
    ) {
        tracing::warn!(error = %e, "failed to emit ai-rewrite event");
    }
}

/// Starts a streaming rewrite of `text`. The frontend supplies `request_id` so
/// it can match `writ://ai-rewrite` events (and cancel) with no window in which
/// an early event — e.g. an immediate connection-refused error — could arrive
/// unmatched. Validation runs synchronously; the network work is spawned.
#[tauri::command]
pub async fn ai_rewrite(
    app: AppHandle,
    request_id: String,
    action: String,
    text: String,
    custom_instruction: Option<String>,
) -> Result<String, String> {
    let cfg = {
        let state = app.state::<AppState>();
        let guard = recover_poison(state.config.lock(), "commands::ai::ai_rewrite");
        guard.ai.clone()
    };

    let prepared = {
        let ai = app.state::<AiState>();
        let memory = recover_poison(ai.keys.lock(), "commands::ai::ai_rewrite");
        prepare_request(&cfg, &action, &text, custom_instruction, |preset| {
            resolve_key(&memory, preset)
        })?
    };

    tracing::info!(text_len = text.len(), "starting rewrite");

    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| sanitize_ai_error(&e.to_string()))?;

    let cancel = Arc::new(AtomicBool::new(false));

    // Register the cancel flag before spawning so a cancel that races the task
    // can never miss it, and so the task's self-removal has an entry to remove.
    {
        let ai = app.state::<AiState>();
        let mut tasks = recover_poison(ai.tasks.lock(), "commands::ai::ai_rewrite");
        tasks.insert(request_id.clone(), cancel.clone());
    }

    let task_app = app.clone();
    let task_id = request_id.clone();
    tauri::async_runtime::spawn(async move {
        run_rewrite_stream(&client, &prepared, &cancel, |event| match event {
            StreamEvent::Chunk(content) => emit_ai(&task_app, &task_id, "chunk", Some(content)),
            StreamEvent::Done => emit_ai(&task_app, &task_id, "done", None),
            StreamEvent::Error(message) => emit_ai(&task_app, &task_id, "error", Some(message)),
        })
        .await;

        let ai = task_app.state::<AiState>();
        let mut tasks = recover_poison(ai.tasks.lock(), "commands::ai::ai_rewrite::cleanup");
        tasks.remove(&task_id);
    });

    Ok(request_id)
}

/// Signals an in-flight stream to stop. Further deltas are dropped and no
/// terminal event is emitted; the frontend already discarded the preview.
#[tauri::command]
pub fn ai_cancel(ai: State<'_, AiState>, request_id: String) {
    let tasks = recover_poison(ai.tasks.lock(), "commands::ai::ai_cancel");
    if let Some(cancel) = tasks.get(&request_id) {
        cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicBool;

    fn base_cfg() -> AiConfig {
        AiConfig {
            enabled: true,
            preset: "ollama".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            model: "llama3".to_string(),
            consented_hosted: false,
        }
    }

    #[test]
    fn parse_sse_extracts_delta_content() {
        match parse_sse_line("data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}") {
            SseLine::Chunk(c) => assert_eq!(c, "hi"),
            _ => panic!("expected chunk"),
        }
    }

    #[test]
    fn parse_sse_recognizes_done_and_ignores_noise() {
        assert!(matches!(parse_sse_line("data: [DONE]"), SseLine::Done));
        assert!(matches!(parse_sse_line(": keep-alive"), SseLine::Ignore));
        assert!(matches!(parse_sse_line(""), SseLine::Ignore));
        assert!(matches!(
            parse_sse_line("data: {\"choices\":[{\"delta\":{}}]}"),
            SseLine::Ignore
        ));
    }

    #[test]
    fn drain_buffers_partial_lines_across_feeds() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"data: {\"choices\":[{\"delta\":{\"content\":\"He");
        assert!(drain_complete_lines(&mut buf).is_empty(), "no newline yet");
        buf.extend_from_slice(b"llo\"}}]}\ndata: [DONE]\n");
        let lines = drain_complete_lines(&mut buf);
        assert_eq!(lines.len(), 2);
        assert!(matches!(parse_sse_line(&lines[0]), SseLine::Chunk(c) if c == "Hello"));
        assert!(matches!(parse_sse_line(&lines[1]), SseLine::Done));
    }

    #[test]
    fn prepare_rejects_disabled() {
        let mut cfg = base_cfg();
        cfg.enabled = false;
        let err = prepare_request(&cfg, "proofread", "x", None, |_| None).unwrap_err();
        assert!(err.contains("turned off"));
    }

    #[test]
    fn prepare_rejects_http_to_remote_host() {
        let mut cfg = base_cfg();
        cfg.base_url = "http://api.groq.com/openai/v1".to_string();
        let err = prepare_request(&cfg, "proofread", "x", None, |_| None).unwrap_err();
        assert!(err.contains("not allowed"), "got: {err}");
    }

    #[test]
    fn prepare_rejects_substring_bypass_host() {
        let mut cfg = base_cfg();
        cfg.base_url = "http://localhost.evil.com/v1".to_string();
        let err = prepare_request(&cfg, "proofread", "x", None, |_| None).unwrap_err();
        assert!(err.contains("not allowed"), "got: {err}");
    }

    #[test]
    fn prepare_rejects_empty_model() {
        let mut cfg = base_cfg();
        cfg.model = "   ".to_string();
        let err = prepare_request(&cfg, "proofread", "x", None, |_| None).unwrap_err();
        assert!(err.contains("model"), "got: {err}");
    }

    #[test]
    fn prepare_local_needs_no_key_or_consent() {
        let cfg = base_cfg();
        let prepared = prepare_request(&cfg, "proofread", "hello", None, |_| None).unwrap();
        assert_eq!(
            prepared.endpoint,
            "http://localhost:11434/v1/chat/completions"
        );
        assert!(prepared.api_key.is_none());
        assert!(prepared.is_localhost);
    }

    #[test]
    fn prepare_hosted_requires_consent_then_key() {
        let mut cfg = base_cfg();
        cfg.preset = "groq".to_string();
        cfg.base_url = "https://api.groq.com/openai/v1".to_string();

        let no_consent = prepare_request(&cfg, "polish", "x", None, |_| Some("k".to_string()));
        assert!(no_consent.unwrap_err().contains("Confirm"));

        cfg.consented_hosted = true;
        let no_key = prepare_request(&cfg, "polish", "x", None, |_| None);
        assert!(no_key.unwrap_err().contains("API key"));

        let ok =
            prepare_request(&cfg, "polish", "x", None, |_| Some("secret".to_string())).unwrap();
        assert_eq!(ok.api_key.as_deref(), Some("secret"));
        assert!(!ok.is_localhost);
    }

    #[test]
    fn prepare_custom_requires_instruction() {
        let cfg = base_cfg();
        let err =
            prepare_request(&cfg, "custom", "x", Some("  ".to_string()), |_| None).unwrap_err();
        assert!(err.to_lowercase().contains("instruction"), "got: {err}");
    }

    #[test]
    fn key_state_prefers_keychain_then_memory() {
        let mut memory = HashMap::new();
        // Nothing anywhere.
        assert_eq!(
            compute_key_state(false, &memory, "groq"),
            AiKeyState {
                is_set: false,
                memory_only: false
            }
        );
        // Memory only → set, but session-scoped.
        memory.insert("groq".to_string(), "sk-x".to_string());
        assert_eq!(
            compute_key_state(false, &memory, "groq"),
            AiKeyState {
                is_set: true,
                memory_only: true
            }
        );
        // Keychain hit wins and is not memory-only, even if memory also has one.
        assert_eq!(
            compute_key_state(true, &memory, "groq"),
            AiKeyState {
                is_set: true,
                memory_only: false
            }
        );
    }

    #[test]
    fn resolve_key_falls_back_to_memory() {
        let mut memory = HashMap::new();
        memory.insert("groq".to_string(), "from-memory".to_string());
        assert_eq!(
            resolve_key_from(Some("from-keychain".to_string()), &memory, "groq").as_deref(),
            Some("from-keychain")
        );
        assert_eq!(
            resolve_key_from(None, &memory, "groq").as_deref(),
            Some("from-memory")
        );
        assert_eq!(resolve_key_from(None, &memory, "deepseek"), None);
    }

    #[test]
    fn sanitize_redacts_urls_and_keeps_status() {
        let out = sanitize_ai_error("error sending request to https://api.groq.com/v1: 500");
        assert!(!out.contains("groq.com"), "leaked: {out}");
        assert!(out.contains("500"));
    }

    /// Spawns a one-shot TCP server that returns `response_body` after the given
    /// status line, then closes the connection. Returns the base URL.
    fn spawn_mock(status_line: &'static str, headers: &'static str, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let response = format!("{status_line}\r\n{headers}\r\n{body}");
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        format!("http://127.0.0.1:{}/v1", addr.port())
    }

    fn run_against(base_url: String, cancel: Arc<AtomicBool>) -> Vec<String> {
        let cfg = AiConfig {
            base_url,
            ..base_cfg()
        };
        let prepared = prepare_request(&cfg, "proofread", "hello", None, |_| None).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_task = events.clone();
        tauri::async_runtime::block_on(async move {
            let client = reqwest::Client::builder()
                .connect_timeout(CONNECT_TIMEOUT)
                .build()
                .unwrap();
            run_rewrite_stream(&client, &prepared, &cancel, |event| {
                let mut ev = events_task.lock().unwrap();
                match event {
                    StreamEvent::Chunk(c) => ev.push(format!("chunk:{c}")),
                    StreamEvent::Done => ev.push("done".to_string()),
                    StreamEvent::Error(m) => ev.push(format!("error:{m}")),
                }
            })
            .await;
        });
        Arc::try_unwrap(events).unwrap().into_inner().unwrap()
    }

    #[test]
    fn streams_chunks_then_done() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
                    data: [DONE]\n\n";
        let base = spawn_mock(
            "HTTP/1.1 200 OK",
            "Content-Type: text/event-stream\r\nConnection: close\r\n",
            body,
        );
        let events = run_against(base, Arc::new(AtomicBool::new(false)));
        assert_eq!(events, vec!["chunk:Hel", "chunk:lo", "done"]);
    }

    #[test]
    fn error_status_surfaces_code() {
        let base = spawn_mock(
            "HTTP/1.1 429 Too Many Requests",
            "Content-Length: 0\r\nConnection: close\r\n",
            "",
        );
        let events = run_against(base, Arc::new(AtomicBool::new(false)));
        assert_eq!(events.len(), 1);
        assert!(events[0].contains("429"), "got: {:?}", events);
    }

    #[test]
    fn precancelled_stream_emits_nothing() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\ndata: [DONE]\n\n";
        let base = spawn_mock(
            "HTTP/1.1 200 OK",
            "Content-Type: text/event-stream\r\nConnection: close\r\n",
            body,
        );
        let events = run_against(base, Arc::new(AtomicBool::new(true)));
        assert!(events.is_empty(), "cancelled stream emitted: {:?}", events);
    }
}
