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
//! - Consent precedes every request to a hosted endpoint, the reachability
//!   probe included: the probe carries the key, so it is gated by
//!   [`probe_gate`] exactly as a rewrite is gated by [`prepare_request`].
//! - API keys never touch `config.toml`, the database, or disk. They are stored
//!   in the OS keychain, or held in memory for the session when the keychain is
//!   unavailable. A key read back from the keychain is also cached in memory for
//!   the session, because each keychain read can raise a system password
//!   prompt; that cache is process-local and dies with the process.
//! - Only lengths and status codes are logged. Prompt text, response text, and
//!   keys never reach the logs or error strings shown to the user.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use writ_core::ai::models::{
    filter_openai_ids, parse_anthropic_page, parse_model_list, sort_ids, ListFamily, ModelListError,
};
use writ_core::ai::providers::{self, ProviderGroup, ProviderInfo};
use writ_core::chat::{self, Provider};
use writ_core::config::AiConfig;
use writ_core::polish::{self, PolishAction, PolishError};

use crate::events::{emit_event, WritFrontendEvent};
use crate::poison::recover_poison;
use crate::state::AppState;

/// Connect timeout: a local Ollama that is not running should fail fast.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Overall request budget for a single rewrite.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// The key stored for one account, from the keychain or from this session's
/// memory. The account is the provider id, so the surface asking for it never
/// reaches another provider's credential.
pub(crate) fn key_for(app: &AppHandle, account: &str) -> Option<String> {
    let ai = app.state::<AiState>();
    let memory = recover_poison(ai.keys.lock(), "commands::ai::key_for");
    resolve_key(&ai, &memory, account)
}

/// Whether a key is stored for one account, and whether it is confined to this
/// session's memory.
pub(crate) fn key_state_for(app: &AppHandle, account: &str) -> AiKeyState {
    let ai = app.state::<AiState>();
    let memory = recover_poison(ai.keys.lock(), "commands::ai::key_state_for");
    key_state(&ai, &memory, account)
}

/// Where a provider key is kept.
///
/// One trait rather than a direct call to the platform store, so the lazy move
/// off the pre-1.0 accounts ([`resolve_stored_key`]) is exercised against an
/// in-memory store instead of a machine's real keychain.
pub trait KeyStore: Send + Sync {
    /// The key stored for `account`, or `None` when there is none. `Err` when
    /// the store is unavailable or access was denied.
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    /// Stores `key` under `account`, replacing whatever was there.
    fn set(&self, account: &str, key: &str) -> Result<(), String>;
    /// Removes `account`. Removing what is not there succeeds.
    fn delete(&self, account: &str) -> Result<(), String>;
    /// Whether a key written here outlives the process. False for the
    /// in-memory store, which is what the key row's session-only warning is.
    fn is_persistent(&self) -> bool;
}

/// The platform credential store.
#[cfg(any(target_os = "macos", target_os = "windows"))]
struct OsKeychain;

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl KeyStore for OsKeychain {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        keychain::get(account)
    }

    fn set(&self, account: &str, key: &str) -> Result<(), String> {
        keychain::set(account, key)
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        keychain::delete(account)
    }

    fn is_persistent(&self) -> bool {
        true
    }
}

/// A store that lives and dies with the process, for a platform with no
/// credential store of its own and for the tests.
#[derive(Default)]
pub struct MemoryKeyStore {
    entries: Mutex<HashMap<String, String>>,
}

impl KeyStore for MemoryKeyStore {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        let entries = recover_poison(self.entries.lock(), "commands::ai::MemoryKeyStore::get");
        Ok(entries.get(account).cloned())
    }

    fn set(&self, account: &str, key: &str) -> Result<(), String> {
        let mut entries = recover_poison(self.entries.lock(), "commands::ai::MemoryKeyStore::set");
        entries.insert(account.to_string(), key.to_string());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        let mut entries =
            recover_poison(self.entries.lock(), "commands::ai::MemoryKeyStore::delete");
        entries.remove(account);
        Ok(())
    }

    fn is_persistent(&self) -> bool {
        false
    }
}

/// The store this platform keeps keys in.
fn platform_key_store() -> Box<dyn KeyStore> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        Box::new(OsKeychain)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Box::new(MemoryKeyStore::default())
    }
}

/// Session-scoped runtime state for rewriting, managed separately from
/// [`AppState`] so the large app initializer stays untouched.
pub struct AiState {
    /// Where keys are kept on this platform.
    store: Box<dyn KeyStore>,
    /// In-memory keys, keyed by provider id, used only when the store is
    /// unavailable or access was denied. Never persisted.
    keys: Mutex<HashMap<String, String>>,
    /// What the store answered for a provider this session: `Some(key)` when
    /// one is stored, `None` when the lookup succeeded and found nothing.
    ///
    /// Every keychain read on macOS can raise a system password prompt, and an
    /// unsigned build gets a fresh prompt after each rebuild because the ACL is
    /// bound to the code signature. Answering "is a key set?" and "give me the
    /// key" from this cache keeps that to at most one prompt per session
    /// instead of one per rewrite. Invalidated whenever a key is set or
    /// cleared. Never persisted, never logged.
    key_cache: Mutex<HashMap<String, Option<String>>>,
    /// Cancel flags for in-flight streams, keyed by the frontend's request id.
    tasks: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl AiState {
    /// Session state backed by this platform's key store.
    pub fn new() -> Self {
        Self::with_store(platform_key_store())
    }

    /// Session state backed by `store`.
    pub fn with_store(store: Box<dyn KeyStore>) -> Self {
        Self {
            store,
            keys: Mutex::new(HashMap::new()),
            key_cache: Mutex::new(HashMap::new()),
            tasks: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for AiState {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether a key is stored for a provider, and whether it is confined to memory
/// for this session (keychain unavailable). Surfaced so the UI can warn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AiKeyState {
    /// A key exists for this provider.
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
    use keyring::{Entry, Error};

    /// Keychain service name under which provider keys are stored. The account
    /// is the provider id, so switching providers keeps independent keys.
    const KEYCHAIN_SERVICE: &str = "com.writ.ai";

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

/// Pure key-state policy: keychain wins; otherwise a memory entry is
/// "set but session-only". Separated from the OS call so it is testable.
fn compute_key_state(
    keychain_hit: bool,
    memory: &HashMap<String, String>,
    account: &str,
) -> AiKeyState {
    if keychain_hit {
        AiKeyState {
            is_set: true,
            memory_only: false,
        }
    } else if memory.contains_key(account) {
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
    account: &str,
) -> Option<String> {
    keychain_value.or_else(|| memory.get(account).cloned())
}

/// The account a pre-1.0 build would have stored this provider's key under.
///
/// Before 1.0 the rewrite path used the bare preset id, which is already the
/// new account name, and the chat pane used a namespace of its own. `None` for
/// a local row, which never had a key to move.
fn legacy_account(provider: &str) -> Option<&'static str> {
    if matches!(
        providers::provider(provider).map(|row| row.group),
        Some(ProviderGroup::Local)
    ) {
        return None;
    }
    match provider {
        "anthropic" => Some("chat:anthropic"),
        _ => Some("chat:openai_compatible"),
    }
}

/// The key stored for a provider, reading the store at most once per session
/// and moving a pre-1.0 chat key onto the new account on the way.
///
/// A cached answer, "there is no key" included, is the whole answer: every
/// keychain read on macOS can raise a system password prompt, so a provider
/// with no key must not re-prompt on every rewrite, and a provider whose new
/// account answers must never reach for a legacy one. A local row reads
/// nothing at all.
///
/// The move is per account and happens on the first read: the key is copied
/// onto the provider id and the old entry deleted. A key under both accounts
/// keeps the new one. A failed copy leaves the old entry where it is, so the
/// key is never lost between two stores, and a failed delete still answers.
/// Nothing here, on any path, puts a key in a log line or an error string.
fn resolve_stored_key(
    store: &dyn KeyStore,
    cache: &Mutex<HashMap<String, Option<String>>>,
    provider: &str,
) -> Result<Option<String>, String> {
    {
        let hit = recover_poison(cache.lock(), "commands::ai::resolve_stored_key");
        if let Some(answer) = hit.get(provider) {
            return Ok(answer.clone());
        }
    }

    let remember = |answer: Option<String>| {
        let mut cache = recover_poison(cache.lock(), "commands::ai::resolve_stored_key");
        cache.insert(provider.to_string(), answer.clone());
        answer
    };

    let Some(legacy) = legacy_account(provider) else {
        return Ok(remember(None));
    };

    if let Some(key) = store.get(provider)? {
        return Ok(remember(Some(key)));
    }

    let Some(key) = store.get(legacy)? else {
        return Ok(remember(None));
    };

    match store.set(provider, &key) {
        Ok(()) => {
            if let Err(reason) = store.delete(legacy) {
                tracing::debug!(error = %reason, "the old key entry could not be removed");
            }
        }
        Err(reason) => {
            tracing::debug!(error = %reason, "the key could not be moved to its new entry");
        }
    }
    Ok(remember(Some(key)))
}

/// [`resolve_stored_key`] against this session's store, with a store failure
/// read as "nothing here" so the caller falls through to the memory map.
fn cached_store_get(ai: &AiState, account: &str) -> Option<String> {
    match resolve_stored_key(ai.store.as_ref(), &ai.key_cache, account) {
        Ok(found) => found,
        Err(reason) => {
            tracing::debug!(error = %reason, "key store read failed; falling back to memory");
            None
        }
    }
}

/// Drops the cached answer for an account, after the stored key changes.
fn invalidate_keychain_cache(ai: &AiState, account: &str) {
    let mut cache = recover_poison(
        ai.key_cache.lock(),
        "commands::ai::invalidate_keychain_cache",
    );
    cache.remove(account);
}

fn key_state(ai: &AiState, memory: &HashMap<String, String>, account: &str) -> AiKeyState {
    match cached_store_get(ai, account) {
        // A store that dies with the process is the session-only warning the
        // key row shows, whichever platform put us on one.
        Some(_) => AiKeyState {
            is_set: true,
            memory_only: !ai.store.is_persistent(),
        },
        None => compute_key_state(false, memory, account),
    }
}

fn resolve_key(ai: &AiState, memory: &HashMap<String, String>, account: &str) -> Option<String> {
    resolve_key_from(cached_store_get(ai, account), memory, account)
}

/// Stores a provider key. Prefers the OS keychain; on failure holds the key in
/// memory for the session and reports that state. Never writes the key to disk
/// and never returns it.
#[tauri::command]
pub fn ai_set_api_key(
    ai: State<'_, AiState>,
    provider: String,
    key: String,
) -> Result<AiKeyState, String> {
    if key.is_empty() {
        return Err("The API key is empty.".to_string());
    }
    let mut memory = recover_poison(ai.keys.lock(), "commands::ai::ai_set_api_key");
    invalidate_keychain_cache(&ai, &provider);
    match ai.store.set(&provider, &key) {
        Ok(()) => {
            memory.remove(&provider);
            Ok(AiKeyState {
                is_set: true,
                memory_only: !ai.store.is_persistent(),
            })
        }
        Err(reason) => {
            // `reason` is a store error; it never contains the key.
            tracing::warn!(error = %reason, "key store unavailable; holding key in memory for this session");
            memory.insert(provider, key);
            Ok(AiKeyState {
                is_set: true,
                memory_only: true,
            })
        }
    }
}

/// Removes a provider key from both the keychain and memory.
#[tauri::command]
pub fn ai_clear_api_key(ai: State<'_, AiState>, provider: String) -> Result<AiKeyState, String> {
    let mut memory = recover_poison(ai.keys.lock(), "commands::ai::ai_clear_api_key");
    memory.remove(&provider);
    invalidate_keychain_cache(&ai, &provider);
    if let Err(reason) = ai.store.delete(&provider) {
        tracing::debug!(error = %reason, "key store delete failed");
    }
    // A key the old build left behind would otherwise be moved onto the
    // account by the next read, undoing the clear.
    if let Some(legacy) = legacy_account(&provider) {
        if let Err(reason) = ai.store.delete(legacy) {
            tracing::debug!(error = %reason, "the old key entry could not be removed");
        }
    }
    Ok(AiKeyState {
        is_set: false,
        memory_only: false,
    })
}

/// Reports whether a key is set for a provider, without returning it.
#[tauri::command]
pub fn ai_has_api_key(ai: State<'_, AiState>, provider: String) -> Result<AiKeyState, String> {
    let memory = recover_poison(ai.keys.lock(), "commands::ai::ai_has_api_key");
    Ok(key_state(&ai, &memory, &provider))
}

// --- Consent ---------------------------------------------------------------

/// What the UI needs to know about the configured endpoint before running a
/// rewrite: where it points, whether it needs consent, and whether it has one.
///
/// The frontend never parses the base URL itself — it renders this — so there
/// is exactly one host-resolution code path in the product.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AiEndpointState {
    /// Resolved host, or `None` when the base URL does not parse.
    pub host: Option<String>,
    /// Host with `:port` when the URL carries one; for display.
    pub host_port: Option<String>,
    /// The endpoint leaves this machine, so consent and a key are required.
    pub is_hosted: bool,
    /// The scheme/host pair passes the outbound guard.
    pub is_allowed: bool,
    /// The send notice has been accepted for this exact host.
    pub is_consented: bool,
    /// Whether a key is stored for the current provider, and where it lives.
    pub key_state: AiKeyState,
    /// The connection's provider id, so the UI names the row it is reporting.
    pub provider: String,
}

/// Whether answering "is a key set?" for this config needs the OS keychain at
/// all. A local endpoint never uses a key, so asking would raise a system
/// password prompt to compute a value nothing reads.
fn needs_key_lookup(cfg: &AiConfig) -> bool {
    polish::resolve_endpoint(&cfg.effective_base_url())
        .map(|t| t.is_hosted)
        .unwrap_or(false)
}

/// Builds the endpoint state for `cfg`. Pure over its key lookup so the
/// consent/key matrix is testable without a keychain.
fn endpoint_state_from(cfg: &AiConfig, key_state: AiKeyState) -> AiEndpointState {
    let provider = cfg.provider.clone();
    match polish::resolve_endpoint(&cfg.effective_base_url()) {
        Ok(target) => AiEndpointState {
            is_consented: !target.is_hosted || is_consented(cfg, &target.host),
            host: Some(target.host),
            host_port: Some(target.host_port),
            is_hosted: target.is_hosted,
            is_allowed: target.is_allowed,
            key_state,
            provider,
        },
        Err(_) => AiEndpointState {
            host: None,
            host_port: None,
            is_hosted: false,
            is_allowed: false,
            is_consented: false,
            key_state,
            provider,
        },
    }
}

/// Reports where the configured endpoint points and what it still needs, so the
/// UI can ask for consent or a key before a rewrite is attempted rather than
/// after it fails.
#[tauri::command]
pub fn ai_endpoint_state(app: AppHandle) -> Result<AiEndpointState, String> {
    let cfg = {
        let state = app.state::<AppState>();
        let guard = recover_poison(state.config.lock(), "commands::ai::ai_endpoint_state");
        guard.ai.clone()
    };
    // A local endpoint needs no key, so do not consult the keychain to answer a
    // question nothing reads: on macOS that alone can raise a password prompt.
    let key_state = if needs_key_lookup(&cfg) {
        let ai = app.state::<AiState>();
        let memory = recover_poison(ai.keys.lock(), "commands::ai::ai_endpoint_state");
        key_state(&ai, &memory, &cfg.provider)
    } else {
        AiKeyState {
            is_set: false,
            memory_only: false,
        }
    };
    Ok(endpoint_state_from(&cfg, key_state))
}

/// Records the send notice for the host the connection reaches.
///
/// Rewriting and the chat share one connection, so there is one host to
/// consent to and the caller names nothing. The host is resolved here rather
/// than supplied by the caller, so consent is always stored under the exact
/// string the guard later checks — a client-computed host could never drift
/// out of agreement with the guard. Refuses a local or disallowed endpoint:
/// there is nothing to consent to.
#[tauri::command]
pub fn ai_consent_host(app: AppHandle) -> Result<AiEndpointState, String> {
    let state = app.state::<AppState>();
    let mut config = {
        let guard = recover_poison(state.config.lock(), "commands::ai::ai_consent_host");
        guard.clone()
    };

    let target =
        polish::resolve_endpoint(&config.ai.effective_base_url()).map_err(|e| e.to_string())?;
    if !target.is_allowed {
        return Err(PolishError::EndpointNotAllowed.to_string());
    }
    if !target.is_hosted {
        return Err("This endpoint is on your machine; nothing is sent.".to_string());
    }

    // Record under the lock and re-read there, so a settings write landing
    // between the read above and this point is not silently overwritten with a
    // stale clone. On a failed disk write the in-memory list is put back, so
    // memory and disk never disagree about what was consented to.
    if !is_consented(&config.ai, &target.host) {
        let updated = {
            let mut guard = recover_poison(state.config.lock(), "commands::ai::ai_consent_host");
            guard.ai.consented_hosts.push(target.host.clone());
            guard.ai.consented_hosts.sort();
            guard.ai.consented_hosts.dedup();
            guard.clone()
        };
        if let Err(reason) = super::config::persist_config(&state, &updated) {
            let mut guard = recover_poison(state.config.lock(), "commands::ai::ai_consent_host");
            guard.ai.consented_hosts.retain(|h| h != &target.host);
            return Err(reason);
        }
        config = updated;
    }

    let key_state = {
        let ai = app.state::<AiState>();
        let memory = recover_poison(ai.keys.lock(), "commands::ai::ai_consent_host");
        // Reached only for a hosted endpoint, which does need a key.
        key_state(&ai, &memory, &config.ai.provider)
    };
    Ok(endpoint_state_from(&config.ai, key_state))
}

// --- Request preparation (pure, testable) ----------------------------------

/// Everything a stream needs, resolved from config and validated. Building this
/// performs every pre-flight check, so the async task only does I/O.
#[derive(Debug, Clone, PartialEq)]
struct PreparedRequest {
    endpoint: String,
    /// The wire the endpoint speaks, which decides the headers and how a
    /// frame of the stream is read.
    provider: Provider,
    body: serde_json::Value,
    api_key: Option<String>,
    is_localhost: bool,
}

/// Validates config + inputs and resolves the request. `lookup_key` maps a
/// provider id to its key (keychain or memory); it is only consulted for
/// hosted endpoints. Errors carry a plain, secret-free message for the UI.
fn prepare_request(
    cfg: &AiConfig,
    action_id: &str,
    text: &str,
    custom_instruction: Option<String>,
    lookup_key: impl FnOnce(&str) -> Option<String>,
) -> Result<PreparedRequest, PolishError> {
    if !cfg.rewrite.enabled {
        return Err(PolishError::Disabled);
    }

    let action = PolishAction::parse(action_id, custom_instruction)?;
    let messages = polish::build_messages(&action, text)?;

    // The one authority: the guard below and `ai_consent_host` resolve the host
    // through the same call, so the string checked here is always the string
    // recorded as consent.
    let base_url = cfg.effective_base_url();
    let target = polish::resolve_endpoint(&base_url)?;
    if !target.is_allowed {
        return Err(PolishError::EndpointNotAllowed);
    }

    if cfg.model.trim().is_empty() {
        return Err(PolishError::ModelRequired);
    }

    let api_key = if target.is_hosted {
        if !is_consented(cfg, &target.host) {
            return Err(PolishError::ConsentRequired {
                host: target.host.clone(),
            });
        }
        match lookup_key(&cfg.provider) {
            Some(k) => Some(k),
            None => {
                return Err(PolishError::ApiKeyRequired {
                    host: target.host.clone(),
                })
            }
        }
    } else {
        None
    };

    let wire = cfg.wire();
    let provider = Provider::from_wire(wire);
    Ok(PreparedRequest {
        endpoint: chat::endpoint(provider, &base_url),
        provider,
        body: polish::build_request_body(wire, &cfg.model, &messages),
        api_key,
        is_localhost: !target.is_hosted,
    })
}

/// Whether the send notice was accepted for `host`. Membership is exact: a
/// consent given for one provider never covers another.
pub(crate) fn is_consented(cfg: &AiConfig, host: &str) -> bool {
    cfg.consented_hosts.iter().any(|h| h == host)
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
    /// The server reported a failure mid-stream. Carries nothing it wrote: an
    /// error frame's fields are response text, which can quote the request.
    Failed,
    Ignore,
}

/// What a rewrite says when the server ends the stream with an error frame.
const STREAM_FAILED: &str = "The model server ended the reply.";

/// Parses a single already-trimmed SSE line.
///
/// The grammar is [`writ_core::chat::parse_delta`]'s: the rewrite stream and
/// the chat pane read the same frames on either wire, so there is one answer
/// to what a line means rather than two that can drift.
fn parse_sse_line(provider: Provider, line: &str) -> SseLine {
    match chat::parse_delta(provider, line) {
        writ_core::chat::Delta::Text(content) => SseLine::Chunk(content),
        writ_core::chat::Delta::Done => SseLine::Done,
        writ_core::chat::Delta::Failed => SseLine::Failed,
        writ_core::chat::Delta::Ignore => SseLine::Ignore,
    }
}

/// Drains every complete (newline-terminated) line from `buf`, leaving any
/// trailing partial line in place. Splitting the byte buffer on `\n` is
/// UTF-8-safe because a newline never appears inside a multibyte sequence, so a
/// chunk boundary mid-character cannot corrupt a decoded line.
pub(crate) fn drain_complete_lines(buf: &mut Vec<u8>) -> Vec<String> {
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
    builder = match (&prepared.api_key, prepared.provider) {
        // The Messages API takes the key in its own header and requires the
        // version it is being called against.
        (Some(key), Provider::Anthropic) => builder
            .header("x-api-key", key)
            .header("anthropic-version", chat::ANTHROPIC_VERSION),
        (Some(key), Provider::OpenAiCompatible) => builder.bearer_auth(key),
        (None, Provider::Anthropic) => builder.header("anthropic-version", chat::ANTHROPIC_VERSION),
        (None, Provider::OpenAiCompatible) => builder,
    };

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
            match parse_sse_line(prepared.provider, &line) {
                SseLine::Chunk(content) => on_event(StreamEvent::Chunk(content)),
                SseLine::Done => {
                    on_event(StreamEvent::Done);
                    return;
                }
                // Ending as Done would hand back an empty rewrite that reads
                // as a model with nothing to say.
                SseLine::Failed => {
                    tracing::warn!("the model server ended the stream with an error frame");
                    on_event(StreamEvent::Error(STREAM_FAILED.to_string()));
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
pub(crate) fn connection_error_message(err: &reqwest::Error, is_localhost: bool) -> String {
    if err.is_connect() && is_localhost {
        return "Could not reach the local model server. Is Ollama running?".to_string();
    }
    sanitize_ai_error(&err.to_string())
}

/// Redacts any URL from an error string so a configured endpoint (which may
/// carry a token in a query) never reaches logs or the UI. Mirrors the update
/// path's redaction; falls back to a generic message when nothing is left.
pub(crate) fn sanitize_ai_error(raw: &str) -> String {
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

/// Installs the `ring` rustls provider as the process default if none is set.
/// Mirrors tauri-plugin-updater's guard so the two never double-install, and
/// guarantees a provider exists even when no update check has run yet — a
/// `rustls-no-provider` client panics at construction without one.
fn ensure_crypto_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
}

/// Builds the rewrite HTTP client. Installs the crypto provider first (a
/// `rustls-no-provider` client panics at construction otherwise). Redirects are
/// refused: following a 3xx would re-send the request body (the user's text) to
/// the `Location` host, escaping the endpoint guard, so a 3xx surfaces as an
/// error status instead.
pub(crate) fn build_client() -> Result<reqwest::Client, String> {
    ensure_crypto_provider();
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| sanitize_ai_error(&e.to_string()))
}

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
            resolve_key(&ai, &memory, preset)
        })
        .map_err(|e| e.to_string())?
    };

    tracing::info!(text_len = text.len(), "starting rewrite");

    let client = build_client()?;

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

/// Timeout for the connection probe (connect and overall).
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// Budget for one model list, every page of it included.
const MODEL_LIST_TIMEOUT: Duration = Duration::from_secs(5);
/// Budget for one knock on a local runtime's port.
const LOCAL_PROBE_TIMEOUT: Duration = Duration::from_secs(1);

/// Upper bound on model ids returned to the UI, so a provider with a huge
/// catalogue never floods the picker.
const MODEL_LIST_CAP: usize = 200;

/// Outcome of probing the configured endpoint's `/models`. `kind` is a machine
/// category the frontend maps to a message; `detail` is a sanitized fragment
/// (host:port or a status code), never a response body.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AiConnectionStatus {
    /// An HTTP response was received (any status).
    pub reachable: bool,
    /// Whether the configured model appears in the server's model list; `None`
    /// when that list is unavailable (auth/non-2xx/unparsable) or no model is
    /// configured.
    pub model_listed: Option<bool>,
    /// One of `ok`, `model_missing`, `unauthorized`, `server_error`, `refused`,
    /// `timeout`, `error`, or one of the three decided before any request is
    /// made: `invalid_url`, `consent_required`, `key_required`.
    pub kind: String,
    /// Sanitized fragment: host:port, a status code, or empty.
    pub detail: String,
    /// Model ids the endpoint advertises (sorted, deduped, capped). Empty unless
    /// a 2xx `/models` response was parsed. These are ids only — no other body
    /// content is read.
    pub models: Vec<String>,
}

impl AiConnectionStatus {
    fn new(reachable: bool, model_listed: Option<bool>, kind: &str, detail: String) -> Self {
        Self {
            reachable,
            model_listed,
            kind: kind.to_string(),
            detail,
            models: Vec::new(),
        }
    }

    fn with_models(mut self, models: Vec<String>) -> Self {
        self.models = models;
        self
    }
}

/// Whether `model` is among `ids`. `None` when `model` is empty or `ids` is
/// empty (the list is unusable for the decision).
fn model_listed_among(ids: &[String], model: &str) -> Option<bool> {
    if model.trim().is_empty() || ids.is_empty() {
        return None;
    }
    Some(ids.iter().any(|id| id == model))
}

/// Builds a client on one time budget. Redirects are refused everywhere for
/// the reason [`build_client`] gives: a 3xx would re-send to the `Location`
/// host, escaping the endpoint guard.
fn build_http_client(budget: Duration) -> Result<reqwest::Client, String> {
    ensure_crypto_provider();
    reqwest::Client::builder()
        .connect_timeout(budget)
        .timeout(budget)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| sanitize_ai_error(&e.to_string()))
}

/// Builds the connection-check client (3s connect + overall).
fn build_probe_client() -> Result<reqwest::Client, String> {
    build_http_client(PROBE_TIMEOUT)
}

// --- Reading a provider's model list ---------------------------------------

/// A transport failure, free of the HTTP client's types so what it means for a
/// model list is decided by a function a test can call with no network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransportFailure {
    /// The request ran out of time.
    Timeout,
    /// The address could not be reached: no route, no name, nothing listening.
    Connect,
    /// Anything else the client refused to do.
    Other,
}

fn transport_failure(err: &reqwest::Error) -> TransportFailure {
    if err.is_timeout() {
        TransportFailure::Timeout
    } else if err.is_connect() || err.is_request() {
        TransportFailure::Connect
    } else {
        TransportFailure::Other
    }
}

/// What a transport failure means for a model list.
fn list_error_for(failure: TransportFailure) -> ModelListError {
    match failure {
        TransportFailure::Timeout => ModelListError::Timeout,
        TransportFailure::Connect | TransportFailure::Other => ModelListError::Unreachable,
    }
}

/// What an HTTP status means for a model list, or `None` when it is a success.
fn list_error_for_status(code: u16) -> Option<ModelListError> {
    match code {
        200..=299 => None,
        401 | 403 => Some(ModelListError::Unauthorized),
        other => Some(ModelListError::Status { code: other }),
    }
}

/// The word a failed list is logged under. Never the provider's own text.
fn list_error_kind(error: &ModelListError) -> &'static str {
    match error {
        ModelListError::Unreachable => "unreachable",
        ModelListError::Timeout => "timeout",
        ModelListError::Unauthorized => "unauthorized",
        ModelListError::Status { .. } => "status",
        ModelListError::Malformed => "malformed",
        ModelListError::ConsentRequired => "consent_required",
    }
}

/// How many ids one page asks for.
const LIST_PAGE_SIZE: u32 = 1000;

/// How many pages of a cursor-paginated list are followed before the rest is
/// left unread. A provider that answers an endless cursor cannot hang the
/// picker.
const MAX_LIST_PAGES: usize = 20;

/// The URL of one page of a model list.
///
/// Gemini's native list takes the key in the query, which is the reason no
/// error or log line in this module may ever carry a URL.
fn list_page_url(
    family: ListFamily,
    list_url: &str,
    api_key: Option<&str>,
    cursor: Option<&str>,
) -> Result<String, ModelListError> {
    if !matches!(family, ListFamily::Anthropic | ListFamily::Gemini) {
        return Ok(list_url.to_string());
    }
    let mut url = url::Url::parse(list_url).map_err(|_| ModelListError::Unreachable)?;
    {
        let mut query = url.query_pairs_mut();
        match family {
            ListFamily::Anthropic => {
                query.append_pair("limit", &LIST_PAGE_SIZE.to_string());
                if let Some(cursor) = cursor {
                    query.append_pair("after_id", cursor);
                }
            }
            _ => {
                query.append_pair("pageSize", &LIST_PAGE_SIZE.to_string());
                if let Some(key) = api_key {
                    query.append_pair("key", key);
                }
                if let Some(cursor) = cursor {
                    query.append_pair("pageToken", cursor);
                }
            }
        }
    }
    Ok(url.to_string())
}

/// Reads one page of a model list. Only the ids are taken from the body.
async fn fetch_list_page(
    client: &reqwest::Client,
    family: ListFamily,
    url: &str,
    api_key: Option<&str>,
) -> Result<String, ModelListError> {
    let mut request = client.get(url);
    request = match (family, api_key) {
        (ListFamily::Anthropic, key) => {
            let request = request.header("anthropic-version", chat::ANTHROPIC_VERSION);
            match key {
                Some(key) => request.header("x-api-key", key),
                None => request,
            }
        }
        // Gemini's key rides the query, so no header is added here.
        (ListFamily::Gemini, _) => request,
        (_, Some(key)) => request.bearer_auth(key),
        (_, None) => request,
    };

    let response = request
        .send()
        .await
        .map_err(|error| list_error_for(transport_failure(&error)))?;
    if let Some(error) = list_error_for_status(response.status().as_u16()) {
        return Err(error);
    }
    response.text().await.map_err(|_| ModelListError::Malformed)
}

/// One page of a list: its ids, and the cursor the next page is asked for
/// with. Only the two paginated families answer a cursor.
fn parse_list_page(
    family: ListFamily,
    body: &str,
) -> Result<(Vec<String>, Option<String>), ModelListError> {
    match family {
        ListFamily::Anthropic => {
            let page = parse_anthropic_page(body)?;
            Ok((page.ids, page.next_cursor))
        }
        ListFamily::Gemini => Ok((
            parse_model_list(ListFamily::Gemini, body)?,
            next_page_token(body),
        )),
        other => Ok((parse_model_list(other, body)?, None)),
    }
}

/// Gemini's cursor, which its list carries beside the models.
fn next_page_token(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .get("nextPageToken")
        .and_then(|token| token.as_str())
        .filter(|token| !token.is_empty())
        .map(String::from)
}

/// Follows a list to its end, page by page.
///
/// The page fetcher is a parameter so the driver is exercised over recorded
/// bodies without a network, and so one implementation serves every family:
/// the families that answer no cursor stop after one page.
async fn collect_list_pages<F, Fut>(
    family: ListFamily,
    mut fetch_page: F,
) -> Result<Vec<String>, ModelListError>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: std::future::Future<Output = Result<String, ModelListError>>,
{
    let mut ids: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_LIST_PAGES {
        let body = fetch_page(cursor.take()).await?;
        let (page, next) = parse_list_page(family, &body)?;
        ids.extend(page);
        match next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    let mut ids = sort_ids(ids);
    ids.dedup();
    Ok(ids)
}

/// The model ids a provider advertises, in the order the picker shows them.
///
/// OpenAI's own catalogue is filtered to what can answer a chat request; every
/// other family is shown whole, because the same words appear in ids that do
/// answer one elsewhere.
async fn fetch_model_ids(
    client: &reqwest::Client,
    provider: &str,
    list_url: &str,
    api_key: Option<&str>,
) -> Result<Vec<String>, ModelListError> {
    let family = ListFamily::for_provider(provider);
    let ids = collect_list_pages(family, |cursor| async move {
        let url = list_page_url(family, list_url, api_key, cursor.as_deref())?;
        fetch_list_page(client, family, &url, api_key).await
    })
    .await?;
    Ok(match provider {
        "openai" => filter_openai_ids(ids),
        _ => ids,
    })
}

/// The provider table, so the settings dropdown, the probe and the model list
/// read one definition of a row rather than three.
#[tauri::command]
pub fn ai_providers() -> Vec<ProviderInfo> {
    providers::PROVIDERS.to_vec()
}

/// Where one model list request is allowed to go.
#[derive(Debug)]
struct ListTargets {
    /// The resolved list endpoint, which decides whether a key is read.
    list: polish::EndpointTarget,
    /// The URL the request is made against.
    list_url: String,
}

/// Whether the model list may be read at all, and from where.
///
/// The list carries the key, so it is a send under ADR-031 rule 2.2 and waits
/// for Allow exactly as a rewrite does. Both the base URL and the list URL go
/// through the endpoint guard and the consent check, because the two differ on
/// one row and a hand-typed base is reachable from both. This is the only
/// place the list URL is produced, so no caller can reach the network around
/// the gate.
fn list_gate(cfg: &AiConfig) -> Result<ListTargets, ModelListError> {
    let base_url = cfg.effective_base_url();
    let base = allowed_target(&base_url)?;
    let list_url =
        providers::models_url_for(&cfg.provider, &base_url).ok_or(ModelListError::Unreachable)?;
    let list = allowed_target(&list_url)?;

    for reached in [&base, &list] {
        if reached.is_hosted && !is_consented(cfg, &reached.host) {
            return Err(ModelListError::ConsentRequired);
        }
    }

    Ok(ListTargets { list, list_url })
}

/// The gate, then the key, in that order.
///
/// Reading a key can raise a system password prompt and puts a credential in
/// memory, so a list that will not be sent must never reach one: `read_key` is
/// called only once [`list_gate`] has allowed the request, and not at all for a
/// local endpoint.
fn gated_list_key<F>(
    cfg: &AiConfig,
    read_key: F,
) -> Result<(ListTargets, Option<String>), ModelListError>
where
    F: FnOnce(&str) -> Option<String>,
{
    let targets = list_gate(cfg)?;
    let api_key = if targets.list.is_hosted {
        read_key(&cfg.provider)
    } else {
        None
    };
    Ok((targets, api_key))
}

/// Reads the model list of the configured connection.
///
/// The list carries the key, so it is a send: a hosted host that has not been
/// allowed answers `ConsentRequired` and nothing leaves the machine. Both the
/// gate and the list URL come from [`list_gate`].
#[tauri::command]
pub async fn ai_list_models(app: AppHandle) -> Result<Vec<String>, ModelListError> {
    let cfg = {
        let state = app.state::<AppState>();
        let guard = recover_poison(state.config.lock(), "commands::ai::ai_list_models");
        guard.ai.clone()
    };

    let (targets, api_key) = gated_list_key(&cfg, |provider| key_for(&app, provider))?;

    let client = build_http_client(MODEL_LIST_TIMEOUT).map_err(|_| ModelListError::Unreachable)?;
    let ids = fetch_model_ids(
        &client,
        &cfg.provider,
        &targets.list_url,
        api_key.as_deref(),
    )
    .await;
    match &ids {
        Ok(ids) => tracing::info!(
            host = %targets.list.host,
            provider = %cfg.provider,
            count = ids.len(),
            "read the model list"
        ),
        Err(error) => tracing::warn!(
            host = %targets.list.host,
            provider = %cfg.provider,
            kind = list_error_kind(error),
            "the model list could not be read"
        ),
    }
    ids
}

/// Resolves a URL the guard allows, or says nothing answered there.
fn allowed_target(url: &str) -> Result<polish::EndpointTarget, ModelListError> {
    match polish::resolve_endpoint(url) {
        Ok(target) if target.is_allowed => Ok(target),
        _ => Err(ModelListError::Unreachable),
    }
}

// --- Knocking on the local runtimes ----------------------------------------

/// Whether each local runtime answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LocalProbe {
    /// Ollama answered on its own port.
    pub ollama: bool,
    /// LM Studio answered on its own port.
    pub lmstudio: bool,
}

/// Ollama's native tag list: it names what is installed without a key.
const OLLAMA_PROBE_URL: &str = "http://127.0.0.1:11434/api/tags";
/// LM Studio's OpenAI-compatible model list, which also needs no key.
const LMSTUDIO_PROBE_URL: &str = "http://127.0.0.1:1234/v1/models";

/// Builds the client the loopback knock uses: no proxy, no key, and no header
/// beyond what the client insists on sending. A loopback request carrying
/// neither a credential nor note text is not a destination (ADR-040 section 4).
fn build_local_probe_client() -> Result<reqwest::Client, String> {
    ensure_crypto_provider();
    reqwest::Client::builder()
        .connect_timeout(LOCAL_PROBE_TIMEOUT)
        .timeout(LOCAL_PROBE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .user_agent("")
        .build()
        .map_err(|e| sanitize_ai_error(&e.to_string()))
}

/// Whether something answered at `url`. Any 2xx is "running"; the body is not
/// read.
async fn knock(client: &reqwest::Client, url: &str) -> bool {
    match client.get(url).send().await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

/// Knocks on both addresses at once. Taken as arguments so the knock is driven
/// against a socket a test owns.
pub async fn probe_local_at(ollama_url: &str, lmstudio_url: &str) -> LocalProbe {
    let Ok(client) = build_local_probe_client() else {
        return LocalProbe {
            ollama: false,
            lmstudio: false,
        };
    };
    let (ollama, lmstudio) =
        futures_util::future::join(knock(&client, ollama_url), knock(&client, lmstudio_url)).await;
    LocalProbe { ollama, lmstudio }
}

/// Reports which local runtimes are running, so the provider rows can say so
/// before one is chosen.
#[tauri::command]
pub async fn ai_probe_local() -> LocalProbe {
    probe_local_at(OLLAMA_PROBE_URL, LMSTUDIO_PROBE_URL).await
}

/// What a failed list is as a connection status. The provider's own words are
/// not among the fields: `detail` is the host, or a status code.
fn check_status_for(error: &ModelListError, host_port: &str) -> AiConnectionStatus {
    match error {
        ModelListError::Timeout => {
            AiConnectionStatus::new(false, None, "timeout", host_port.to_string())
        }
        ModelListError::Unreachable => {
            AiConnectionStatus::new(false, None, "refused", host_port.to_string())
        }
        ModelListError::Unauthorized => {
            AiConnectionStatus::new(true, None, "unauthorized", host_port.to_string())
        }
        ModelListError::Status { code } => {
            AiConnectionStatus::new(true, None, "server_error", code.to_string())
        }
        ModelListError::Malformed => {
            AiConnectionStatus::new(true, None, "error", host_port.to_string())
        }
        ModelListError::ConsentRequired => {
            AiConnectionStatus::new(false, None, "consent_required", host_port.to_string())
        }
    }
}

/// Reads the connection's model list and answers what it says about the
/// configured model. The list is the same one [`ai_list_models`] reads, so the
/// check cannot pass against a request the picker never makes.
async fn run_connection_check(
    client: &reqwest::Client,
    provider: &str,
    list_url: &str,
    api_key: Option<&str>,
    model: &str,
    host_port: &str,
) -> AiConnectionStatus {
    let mut ids = match fetch_model_ids(client, provider, list_url, api_key).await {
        Ok(ids) => ids,
        Err(error) => return check_status_for(&error, host_port),
    };
    ids.truncate(MODEL_LIST_CAP);

    let status = match model_listed_among(&ids, model) {
        Some(true) => AiConnectionStatus::new(true, Some(true), "ok", String::new()),
        Some(false) => {
            AiConnectionStatus::new(true, Some(false), "model_missing", model.to_string())
        }
        None => AiConnectionStatus::new(true, None, "ok", String::new()),
    };
    status.with_models(ids)
}

/// Whether the probe may contact `target`.
///
/// The probe carries the API key, so it is a request to the provider like any
/// other and passes the gate [`prepare_request`] applies before text is sent:
/// the host must be consented to. The connection is checked from the settings
/// section whether or not either feature is switched on, so no feature switch
/// is read here. A local endpoint reaches nobody and stays ungated.
fn probe_gate(cfg: &AiConfig, target: &polish::EndpointTarget) -> Result<(), PolishError> {
    if !target.is_hosted {
        return Ok(());
    }
    if !is_consented(cfg, &target.host) {
        return Err(PolishError::ConsentRequired {
            host: target.host.clone(),
        });
    }
    Ok(())
}

/// The status for a probe that was never sent. `detail` is the bare host: it
/// names the consent key the user would be granting, not the probe target.
///
/// The `kind` strings are the frontend's contract — it renders these as a
/// pre-request state rather than as a connection failure.
fn blocked_probe_status(err: &PolishError, host: &str) -> AiConnectionStatus {
    let kind = match err {
        PolishError::ConsentRequired { .. } => "consent_required",
        _ => "error",
    };
    AiConnectionStatus::new(false, None, kind, host.to_string())
}

/// Probes the configured endpoint's `/models` so the UI can show connection
/// state before a rewrite is attempted. No key is sent to a local endpoint; a
/// hosted one gets the stored key, and only once the user has consented to that
/// host. Response bodies are read only for model ids.
#[tauri::command]
pub async fn ai_check_connection(app: AppHandle) -> Result<AiConnectionStatus, String> {
    let cfg = {
        let state = app.state::<AppState>();
        let guard = recover_poison(state.config.lock(), "commands::ai::ai_check_connection");
        guard.ai.clone()
    };

    // Same resolver as the rewrite guard and the consent recorder — the probe
    // must never disagree with them about where the endpoint points.
    let base_url = cfg.effective_base_url();
    let target = match polish::resolve_endpoint(&base_url) {
        Ok(t) if t.is_allowed => t,
        _ => {
            return Ok(AiConnectionStatus::new(
                false,
                None,
                "invalid_url",
                String::new(),
            ))
        }
    };

    // Before the key is read, so a blocked probe raises no keychain prompt and
    // no credential exists to leak.
    if let Err(err) = probe_gate(&cfg, &target) {
        return Ok(blocked_probe_status(&err, &target.host));
    }

    let host_port = target.host_port.clone();
    let api_key = if target.is_hosted {
        let ai = app.state::<AiState>();
        let memory = recover_poison(ai.keys.lock(), "commands::ai::ai_check_connection");
        resolve_key(&ai, &memory, &cfg.provider)
    } else {
        None
    };
    // A hosted provider with no key would answer 401 and read as a broken
    // endpoint; the row is a missing key and says so.
    if target.is_hosted && api_key.is_none() {
        return Ok(AiConnectionStatus::new(
            false,
            None,
            "key_required",
            target.host.clone(),
        ));
    }

    let Some(list_url) = providers::models_url_for(&cfg.provider, &base_url) else {
        return Ok(AiConnectionStatus::new(
            false,
            None,
            "invalid_url",
            String::new(),
        ));
    };
    // The guard runs on the list URL too: one row lists from a host of its own,
    // and a hand-typed base reaches both.
    if allowed_target(&list_url).is_err() {
        return Ok(AiConnectionStatus::new(
            false,
            None,
            "invalid_url",
            String::new(),
        ));
    }

    let client = match build_probe_client() {
        Ok(c) => c,
        Err(detail) => return Ok(AiConnectionStatus::new(false, None, "error", detail)),
    };

    Ok(run_connection_check(
        &client,
        &cfg.provider,
        &list_url,
        api_key.as_deref(),
        cfg.model.trim(),
        &host_port,
    )
    .await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicBool;

    fn base_cfg() -> AiConfig {
        AiConfig {
            provider: "ollama".to_string(),
            base_url: String::new(),
            model: "llama3".to_string(),
            consented_hosts: Vec::new(),
            rewrite: writ_core::config::AiRewriteConfig { enabled: true },
            chat: writ_core::config::AiChatConfig::default(),
        }
    }

    /// A connection pointed at a hand-typed endpoint, for the guard tests that
    /// need a URL no table row carries.
    fn custom_cfg(base_url: &str) -> AiConfig {
        AiConfig {
            provider: "custom".to_string(),
            base_url: base_url.to_string(),
            ..base_cfg()
        }
    }

    #[test]
    fn parse_sse_extracts_delta_content() {
        match parse_sse_line(
            Provider::OpenAiCompatible,
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}",
        ) {
            SseLine::Chunk(c) => assert_eq!(c, "hi"),
            _ => panic!("expected chunk"),
        }
    }

    #[test]
    fn parse_sse_recognizes_done_and_ignores_noise() {
        assert!(matches!(
            parse_sse_line(Provider::OpenAiCompatible, "data: [DONE]"),
            SseLine::Done
        ));
        assert!(matches!(
            parse_sse_line(Provider::OpenAiCompatible, ": keep-alive"),
            SseLine::Ignore
        ));
        assert!(matches!(
            parse_sse_line(Provider::OpenAiCompatible, ""),
            SseLine::Ignore
        ));
        assert!(matches!(
            parse_sse_line(
                Provider::OpenAiCompatible,
                "data: {\"choices\":[{\"delta\":{}}]}"
            ),
            SseLine::Ignore
        ));
    }

    #[test]
    fn parse_sse_reads_a_recorded_error_frame_as_a_failure() {
        // The same recorded frames the chat pane reads its grammar against:
        // one rewrite and one chat request against the same server must not
        // disagree about what an error frame means.
        const OPENAI_ERROR_STREAM: &str =
            include_str!("../../../crates/writ-core/tests/fixtures/chat/openai-error.sse");
        let failures = OPENAI_ERROR_STREAM
            .lines()
            .filter(|line| {
                matches!(
                    parse_sse_line(Provider::OpenAiCompatible, line),
                    SseLine::Failed
                )
            })
            .count();
        assert_eq!(failures, 2, "both spellings of the frame end the stream");

        // Nothing the server wrote is carried out: the sentence a rewrite
        // shows is fixed, and the token in the fixture is in neither.
        assert!(!STREAM_FAILED.contains("ZZ-server-text-that-must-never-be-logged"));
        assert!(matches!(
            parse_sse_line(Provider::OpenAiCompatible, "data: {\"error\":null}"),
            SseLine::Ignore
        ));
        assert!(matches!(
            parse_sse_line(Provider::OpenAiCompatible, "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}],\"error\":null}"),
            SseLine::Chunk(c) if c == "hi"
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
        assert!(
            matches!(parse_sse_line(Provider::OpenAiCompatible, &lines[0]), SseLine::Chunk(c) if c == "Hello")
        );
        assert!(matches!(
            parse_sse_line(Provider::OpenAiCompatible, &lines[1]),
            SseLine::Done
        ));
    }

    #[test]
    fn prepare_rejects_disabled() {
        let mut cfg = base_cfg();
        cfg.rewrite.enabled = false;
        let err = prepare_request(&cfg, "proofread", "x", None, |_| None).unwrap_err();
        assert_eq!(err, PolishError::Disabled);
    }

    #[test]
    fn prepare_rejects_http_to_remote_host() {
        let cfg = custom_cfg("http://api.groq.com/openai/v1");
        let err = prepare_request(&cfg, "proofread", "x", None, |_| None).unwrap_err();
        assert_eq!(err, PolishError::EndpointNotAllowed);
    }

    #[test]
    fn prepare_rejects_substring_bypass_host() {
        let cfg = custom_cfg("http://localhost.evil.com/v1");
        let err = prepare_request(&cfg, "proofread", "x", None, |_| None).unwrap_err();
        assert_eq!(err, PolishError::EndpointNotAllowed);
    }

    #[test]
    fn prepare_rejects_empty_model() {
        let mut cfg = base_cfg();
        cfg.model = "   ".to_string();
        let err = prepare_request(&cfg, "proofread", "x", None, |_| None).unwrap_err();
        assert_eq!(err, PolishError::ModelRequired);
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
        cfg.provider = "groq".to_string();

        let no_consent = prepare_request(&cfg, "polish", "x", None, |_| Some("k".to_string()));
        assert_eq!(
            no_consent.unwrap_err(),
            PolishError::ConsentRequired {
                host: "api.groq.com".to_string()
            }
        );

        // Consent to a different host must not cover this one.
        cfg.consented_hosts = vec!["api.deepseek.com".to_string()];
        let wrong_host = prepare_request(&cfg, "polish", "x", None, |_| Some("k".to_string()));
        assert_eq!(
            wrong_host.unwrap_err(),
            PolishError::ConsentRequired {
                host: "api.groq.com".to_string()
            }
        );

        cfg.consented_hosts = vec!["api.groq.com".to_string()];
        let no_key = prepare_request(&cfg, "polish", "x", None, |_| None);
        assert_eq!(
            no_key.unwrap_err(),
            PolishError::ApiKeyRequired {
                host: "api.groq.com".to_string()
            }
        );

        let ok =
            prepare_request(&cfg, "polish", "x", None, |_| Some("secret".to_string())).unwrap();
        assert_eq!(ok.api_key.as_deref(), Some("secret"));
        assert!(!ok.is_localhost);
    }

    #[test]
    fn an_anthropic_connection_prepares_the_messages_api() {
        let mut cfg = base_cfg();
        cfg.provider = "anthropic".to_string();
        cfg.model = "claude-sonnet-5".to_string();
        cfg.consented_hosts = vec!["api.anthropic.com".to_string()];

        let prepared = prepare_request(&cfg, "proofread", "teh text", None, |account| {
            assert_eq!(account, "anthropic");
            Some("sk-ant".to_string())
        })
        .expect("prepared");

        assert_eq!(prepared.endpoint, "https://api.anthropic.com/v1/messages");
        assert_eq!(prepared.provider, Provider::Anthropic);
        assert!(prepared.body["system"].as_str().is_some());
        assert_eq!(prepared.body["max_tokens"], chat::ANTHROPIC_MAX_TOKENS);
        assert_eq!(prepared.body["messages"][0]["role"], "user");
    }

    #[test]
    fn the_messages_api_is_called_with_its_own_headers_and_no_bearer() {
        let (base, seen) = spawn_recording_mock(
            "HTTP/1.1 200 OK",
            "Content-Type: text/event-stream\r\nConnection: close\r\n",
            "data: [DONE]\n\n",
        );
        let mut cfg = custom_cfg(&base);
        cfg.model = "claude-sonnet-5".to_string();
        let prepared = PreparedRequest {
            provider: Provider::Anthropic,
            endpoint: chat::endpoint(Provider::Anthropic, &base),
            body: serde_json::json!({ "model": "claude-sonnet-5" }),
            api_key: Some(SECRET_KEY.to_string()),
            is_localhost: true,
        };
        drain_stream(&prepared, Arc::new(AtomicBool::new(false)));

        let request = seen.lock().expect("seen").clone();
        let lowered = request.to_lowercase();
        assert!(lowered.contains("x-api-key:"), "{request}");
        assert!(
            lowered.contains("anthropic-version: 2023-06-01"),
            "{request}"
        );
        assert!(
            !lowered.contains("authorization:"),
            "the Messages API was called with a bearer token"
        );
    }

    #[test]
    fn the_openai_wire_still_carries_a_bearer_and_nothing_else() {
        let (base, seen) = spawn_recording_mock(
            "HTTP/1.1 200 OK",
            "Content-Type: text/event-stream\r\nConnection: close\r\n",
            "data: [DONE]\n\n",
        );
        let mut cfg = custom_cfg(&base);
        cfg.consented_hosts = vec![];
        let prepared = PreparedRequest {
            provider: Provider::OpenAiCompatible,
            endpoint: chat::endpoint(Provider::OpenAiCompatible, &base),
            body: serde_json::json!({ "model": cfg.model }),
            api_key: Some(SECRET_KEY.to_string()),
            is_localhost: true,
        };
        drain_stream(&prepared, Arc::new(AtomicBool::new(false)));

        let request = seen.lock().expect("seen").clone();
        let lowered = request.to_lowercase();
        assert!(
            request.starts_with("POST /v1/chat/completions"),
            "{request}"
        );
        assert!(lowered.contains("authorization: bearer"), "{request}");
        assert!(!lowered.contains("x-api-key:"), "{request}");
    }

    #[test]
    fn a_recorded_messages_stream_reads_as_the_chat_reads_it() {
        let (base, _seen) = spawn_recording_mock(
            "HTTP/1.1 200 OK",
            "Content-Type: text/event-stream\r\nConnection: close\r\n",
            ANTHROPIC_STREAM,
        );
        let prepared = PreparedRequest {
            provider: Provider::Anthropic,
            endpoint: chat::endpoint(Provider::Anthropic, &base),
            body: serde_json::json!({ "model": "claude-sonnet-5" }),
            api_key: None,
            is_localhost: true,
        };
        let events = drain_stream(&prepared, Arc::new(AtomicBool::new(false)));

        // What the chat pane's parser makes of the same frames.
        let expected: String = ANTHROPIC_STREAM
            .lines()
            .filter_map(
                |line| match chat::parse_delta(Provider::Anthropic, line.trim()) {
                    writ_core::chat::Delta::Text(text) => Some(text),
                    _ => None,
                },
            )
            .collect();
        let streamed: String = events
            .iter()
            .filter_map(|event| event.strip_prefix("chunk:"))
            .collect();
        assert_eq!(streamed, expected);
        assert!(!expected.is_empty(), "the fixture carries no text");
        assert_eq!(events.last().map(String::as_str), Some("done"));
    }

    #[test]
    fn prepare_custom_requires_instruction() {
        let cfg = base_cfg();
        let err =
            prepare_request(&cfg, "custom", "x", Some("  ".to_string()), |_| None).unwrap_err();
        assert_eq!(err, PolishError::EmptyInstruction);
    }

    #[test]
    fn a_local_endpoint_never_needs_the_keychain() {
        // Reading the keychain can raise a system password prompt on macOS.
        // Asking for a key a local endpoint will never use is a prompt for
        // nothing.
        assert!(!needs_key_lookup(&base_cfg()));
    }

    #[test]
    fn a_hosted_endpoint_needs_the_keychain() {
        let mut cfg = base_cfg();
        cfg.provider = "deepseek".to_string();
        assert!(needs_key_lookup(&cfg));
    }

    #[test]
    fn an_unparseable_url_needs_no_keychain_lookup() {
        assert!(!needs_key_lookup(&custom_cfg("not a url")));
    }

    #[test]
    fn the_table_decides_where_a_request_goes() {
        // A base URL left over from an older file is read for `custom` only,
        // so a stale line cannot redirect a provider the table knows.
        let mut cfg = base_cfg();
        cfg.provider = "groq".to_string();
        cfg.base_url = "http://elsewhere.example".to_string();
        let prepared =
            prepare_request(&cfg, "polish", "x", None, |_| Some("k".to_string())).unwrap_err();
        assert_eq!(
            prepared,
            PolishError::ConsentRequired {
                host: "api.groq.com".to_string()
            }
        );
    }

    #[test]
    fn the_keychain_is_read_once_per_account_then_served_from_cache() {
        let ai = AiState::default();
        // Seed the cache as a successful lookup would.
        {
            let mut cache = ai.key_cache.lock().unwrap();
            cache.insert("groq".to_string(), Some("k".to_string()));
        }
        let memory = HashMap::new();
        // Both readers answer from the cache: no second OS call, so no second
        // password prompt during a session.
        assert_eq!(resolve_key(&ai, &memory, "groq"), Some("k".to_string()));
        assert!(key_state(&ai, &memory, "groq").is_set);
    }

    #[test]
    fn a_cached_absence_is_honoured_without_asking_again() {
        let ai = AiState::default();
        {
            let mut cache = ai.key_cache.lock().unwrap();
            cache.insert("groq".to_string(), None);
        }
        let memory = HashMap::new();
        assert_eq!(resolve_key(&ai, &memory, "groq"), None);
        assert!(!key_state(&ai, &memory, "groq").is_set);
    }

    #[test]
    fn the_memory_fallback_still_wins_when_the_keychain_holds_nothing() {
        let ai = AiState::default();
        {
            let mut cache = ai.key_cache.lock().unwrap();
            cache.insert("groq".to_string(), None);
        }
        let mut memory = HashMap::new();
        memory.insert("groq".to_string(), "from-memory".to_string());
        assert_eq!(
            resolve_key(&ai, &memory, "groq"),
            Some("from-memory".to_string())
        );
        let state = key_state(&ai, &memory, "groq");
        assert!(state.is_set);
        assert!(state.memory_only);
    }

    #[test]
    fn changing_a_key_drops_the_cached_answer() {
        let ai = AiState::default();
        {
            let mut cache = ai.key_cache.lock().unwrap();
            cache.insert("groq".to_string(), Some("old".to_string()));
        }
        invalidate_keychain_cache(&ai, "groq");
        assert!(!ai.key_cache.lock().unwrap().contains_key("groq"));
    }

    #[test]
    fn endpoint_state_reports_consent_and_key_for_a_hosted_provider() {
        let mut cfg = base_cfg();
        cfg.provider = "deepseek".to_string();
        let no_key = AiKeyState {
            is_set: false,
            memory_only: false,
        };

        // The operator's reported state: hosted, allowed, no consent recorded.
        let state = endpoint_state_from(&cfg, no_key);
        assert_eq!(state.host.as_deref(), Some("api.deepseek.com"));
        assert_eq!(state.provider, "deepseek");
        assert!(state.is_hosted);
        assert!(state.is_allowed);
        assert!(!state.is_consented);

        cfg.consented_hosts = vec!["api.deepseek.com".to_string()];
        assert!(endpoint_state_from(&cfg, no_key).is_consented);
    }

    #[test]
    fn endpoint_state_treats_local_as_already_consented() {
        // Nothing leaves the machine, so the UI must never ask.
        let cfg = base_cfg();
        let state = endpoint_state_from(
            &cfg,
            AiKeyState {
                is_set: false,
                memory_only: false,
            },
        );
        assert!(!state.is_hosted);
        assert!(state.is_consented);
    }

    #[test]
    fn endpoint_state_survives_an_unparseable_url() {
        let cfg = custom_cfg("not a url");
        let state = endpoint_state_from(
            &cfg,
            AiKeyState {
                is_set: false,
                memory_only: false,
            },
        );
        assert!(state.host.is_none());
        assert!(!state.is_allowed);
        assert!(!state.is_consented);
    }

    #[test]
    fn consent_is_recorded_under_the_host_the_guard_checks() {
        // The whole point of resolving server-side: whatever `ai_consent_host`
        // would store must satisfy `prepare_request` on the very next call.
        let mut cfg = custom_cfg("  https://API.DeepSeek.com/v1/  ");

        let target = polish::resolve_endpoint(&cfg.effective_base_url()).unwrap();
        cfg.consented_hosts = vec![target.host];

        let prepared = prepare_request(&cfg, "polish", "x", None, |_| Some("k".to_string()));
        assert!(prepared.is_ok(), "got: {:?}", prepared.unwrap_err());
    }

    /// A store that refuses to be asked about a pre-1.0 chat account, so a
    /// read that should never reach for one fails loudly.
    struct NoLegacyReads {
        inner: MemoryKeyStore,
    }

    impl KeyStore for NoLegacyReads {
        fn get(&self, account: &str) -> Result<Option<String>, String> {
            assert!(
                !account.starts_with("chat:"),
                "the old entry was read when the new one answered"
            );
            self.inner.get(account)
        }

        fn set(&self, account: &str, key: &str) -> Result<(), String> {
            self.inner.set(account, key)
        }

        fn delete(&self, account: &str) -> Result<(), String> {
            self.inner.delete(account)
        }

        fn is_persistent(&self) -> bool {
            false
        }
    }

    /// A store whose deletes always fail, for the half-finished move.
    struct DeletesFail {
        inner: MemoryKeyStore,
    }

    impl KeyStore for DeletesFail {
        fn get(&self, account: &str) -> Result<Option<String>, String> {
            self.inner.get(account)
        }

        fn set(&self, account: &str, key: &str) -> Result<(), String> {
            self.inner.set(account, key)
        }

        fn delete(&self, _account: &str) -> Result<(), String> {
            Err("the entry is locked".to_string())
        }

        fn is_persistent(&self) -> bool {
            false
        }
    }

    fn empty_cache() -> Mutex<HashMap<String, Option<String>>> {
        Mutex::new(HashMap::new())
    }

    fn stored(store: &dyn KeyStore, account: &str) -> Option<String> {
        store.get(account).expect("the store answered")
    }

    #[test]
    fn an_anthropic_key_moves_off_the_old_chat_entry() {
        let store = MemoryKeyStore::default();
        store.set("chat:anthropic", "sk-ant").expect("seeded");

        let cache = empty_cache();
        assert_eq!(
            resolve_stored_key(&store, &cache, "anthropic"),
            Ok(Some("sk-ant".to_string()))
        );
        assert_eq!(stored(&store, "anthropic").as_deref(), Some("sk-ant"));
        assert_eq!(stored(&store, "chat:anthropic"), None);
    }

    #[test]
    fn a_hosted_key_moves_off_the_shared_old_chat_entry() {
        let store = MemoryKeyStore::default();
        store
            .set("chat:openai_compatible", "sk-groq")
            .expect("seeded");

        let cache = empty_cache();
        assert_eq!(
            resolve_stored_key(&store, &cache, "groq"),
            Ok(Some("sk-groq".to_string()))
        );
        assert_eq!(stored(&store, "groq").as_deref(), Some("sk-groq"));
        assert_eq!(stored(&store, "chat:openai_compatible"), None);
    }

    #[test]
    fn a_typed_endpoint_moves_off_the_same_old_chat_entry() {
        let store = MemoryKeyStore::default();
        store
            .set("chat:openai_compatible", "sk-custom")
            .expect("seeded");

        let cache = empty_cache();
        assert_eq!(
            resolve_stored_key(&store, &cache, "custom"),
            Ok(Some("sk-custom".to_string()))
        );
        assert_eq!(stored(&store, "custom").as_deref(), Some("sk-custom"));
        assert_eq!(stored(&store, "chat:openai_compatible"), None);
    }

    #[test]
    fn a_key_under_both_entries_keeps_the_new_one() {
        let store = MemoryKeyStore::default();
        store.set("groq", "sk-new").expect("seeded");
        store
            .set("chat:openai_compatible", "sk-old")
            .expect("seeded");

        let cache = empty_cache();
        assert_eq!(
            resolve_stored_key(&store, &cache, "groq"),
            Ok(Some("sk-new".to_string()))
        );
        // The old entry is another provider\'s key as often as it is this
        // one\'s, so a read that did not need it leaves it alone.
        assert_eq!(
            stored(&store, "chat:openai_compatible").as_deref(),
            Some("sk-old")
        );
    }

    #[test]
    fn the_old_entry_is_never_read_when_the_new_one_answers() {
        let store = NoLegacyReads {
            inner: MemoryKeyStore::default(),
        };
        store.set("groq", "sk-new").expect("seeded");

        let cache = empty_cache();
        assert_eq!(
            resolve_stored_key(&store, &cache, "groq"),
            Ok(Some("sk-new".to_string()))
        );
    }

    #[test]
    fn a_local_provider_never_reads_the_store() {
        let store = NoLegacyReads {
            inner: MemoryKeyStore::default(),
        };
        // Would panic on a `chat:` read, and answers nothing for a local row
        // either: a machine that never used AI must see no password prompt.
        store
            .set("ollama", "sk-should-not-be-read")
            .expect("seeded");

        let cache = empty_cache();
        assert_eq!(resolve_stored_key(&store, &cache, "ollama"), Ok(None));
        assert_eq!(resolve_stored_key(&store, &cache, "lmstudio"), Ok(None));
    }

    #[test]
    fn a_move_that_cannot_delete_the_old_entry_still_answers() {
        let store = DeletesFail {
            inner: MemoryKeyStore::default(),
        };
        store.set("chat:anthropic", "sk-ant").expect("seeded");

        let cache = empty_cache();
        assert_eq!(
            resolve_stored_key(&store, &cache, "anthropic"),
            Ok(Some("sk-ant".to_string()))
        );
        assert_eq!(stored(&store, "anthropic").as_deref(), Some("sk-ant"));
    }

    #[test]
    fn a_moved_key_is_answered_from_the_cache_next_time() {
        let store = MemoryKeyStore::default();
        store.set("chat:anthropic", "sk-ant").expect("seeded");
        let cache = empty_cache();

        assert_eq!(
            resolve_stored_key(&store, &cache, "anthropic"),
            Ok(Some("sk-ant".to_string()))
        );
        // The move already happened, so a second read finds the new entry and
        // asks the OS nothing.
        store.delete("anthropic").expect("removed behind the cache");
        assert_eq!(
            resolve_stored_key(&store, &cache, "anthropic"),
            Ok(Some("sk-ant".to_string()))
        );
    }

    #[test]
    fn the_old_entry_names_the_chat_namespace_for_every_hosted_row() {
        assert_eq!(legacy_account("anthropic"), Some("chat:anthropic"));
        for id in ["openai", "gemini", "openrouter", "groq", "custom"] {
            assert_eq!(legacy_account(id), Some("chat:openai_compatible"), "{id}");
        }
        for id in ["ollama", "lmstudio"] {
            assert_eq!(legacy_account(id), None, "{id}");
        }
    }

    #[test]
    fn a_key_in_a_store_that_dies_with_the_session_is_reported_as_such() {
        let ai = AiState::with_store(Box::new(MemoryKeyStore::default()));
        ai.store.set("groq", "sk-x").expect("seeded");
        let memory = HashMap::new();
        let state = key_state(&ai, &memory, "groq");
        assert!(state.is_set);
        assert!(state.memory_only, "a session-only store must say so");
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

    /// The frames `writ-core` reads its Messages-API grammar against, served
    /// over a socket so the rewrite stream meets the real thing.
    const ANTHROPIC_STREAM: &str =
        include_str!("../../../crates/writ-core/tests/fixtures/chat/anthropic-stream.sse");

    /// A key that must never reach a header it does not belong in.
    const SECRET_KEY: &str = "ZZ-rewrite-key";

    /// A mock that hands back the request bytes it read.
    fn spawn_recording_mock(
        status_line: &'static str,
        headers: &'static str,
        body: &'static str,
    ) -> (String, Arc<Mutex<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let seen = Arc::new(Mutex::new(String::new()));
        let recorder = seen.clone();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 8192];
                if let Ok(read) = stream.read(&mut buf) {
                    *recorder.lock().expect("recorder") =
                        String::from_utf8_lossy(&buf[..read]).to_string();
                }
                let _ =
                    stream.write_all(format!("{status_line}\r\n{headers}\r\n{body}").as_bytes());
                let _ = stream.flush();
            }
        });
        (format!("http://127.0.0.1:{}/v1", addr.port()), seen)
    }

    /// Runs one prepared request to the end and collects what it emitted.
    fn drain_stream(prepared: &PreparedRequest, cancel: Arc<AtomicBool>) -> Vec<String> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        tauri::async_runtime::block_on(async move {
            let client = build_client().expect("client");
            run_rewrite_stream(&client, prepared, &cancel, |event| {
                let mut collected = sink.lock().expect("events");
                match event {
                    StreamEvent::Chunk(text) => collected.push(format!("chunk:{text}")),
                    StreamEvent::Done => collected.push("done".to_string()),
                    StreamEvent::Error(message) => collected.push(format!("error:{message}")),
                }
            })
            .await;
        });
        Arc::try_unwrap(events)
            .expect("one reference")
            .into_inner()
            .expect("events")
    }

    fn run_against(base_url: String, cancel: Arc<AtomicBool>) -> Vec<String> {
        let cfg = custom_cfg(&base_url);
        let prepared = prepare_request(&cfg, "proofread", "hello", None, |_| None).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_task = events.clone();
        tauri::async_runtime::block_on(async move {
            let client = build_client().unwrap();
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
    fn refuses_redirect_and_never_resends_body() {
        // The redirect target records whether it ever receives a connection.
        let hit_target = Arc::new(AtomicBool::new(false));
        let listener_target = TcpListener::bind("127.0.0.1:0").unwrap();
        let port_target = listener_target.local_addr().unwrap().port();
        {
            let hit_target = hit_target.clone();
            std::thread::spawn(move || {
                if let Ok((mut stream, _)) = listener_target.accept() {
                    hit_target.store(true, Ordering::SeqCst);
                    let mut buf = [0u8; 2048];
                    let _ = stream.read(&mut buf);
                    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
                }
            });
        }

        // The configured endpoint answers 307 pointing at the target host.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let resp = format!(
                    "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:{port_target}/v1/chat/completions\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            }
        });

        let base = format!("http://127.0.0.1:{port}/v1");
        let events = run_against(base, Arc::new(AtomicBool::new(false)));

        assert!(
            !hit_target.load(Ordering::SeqCst),
            "request body was re-sent to the redirect target"
        );
        assert_eq!(events.len(), 1);
        assert!(
            events[0].contains("307"),
            "expected a 307 error, got: {events:?}"
        );
    }

    fn check_against(base_url: &str, model: &str) -> AiConnectionStatus {
        let list_url = format!("{}/models", base_url.trim_end_matches('/'));
        tauri::async_runtime::block_on(async move {
            let client = build_probe_client().unwrap();
            run_connection_check(&client, "custom", &list_url, None, model, "127.0.0.1:0").await
        })
    }

    fn gate_for(cfg: &AiConfig) -> Result<(), PolishError> {
        let target = polish::resolve_endpoint(&cfg.effective_base_url()).unwrap();
        probe_gate(cfg, &target)
    }

    fn hosted_cfg() -> AiConfig {
        AiConfig {
            provider: "groq".to_string(),
            ..base_cfg()
        }
    }

    #[test]
    fn unconsented_hosted_probe_is_rejected() {
        let mut cfg = hosted_cfg();
        assert_eq!(
            gate_for(&cfg).unwrap_err(),
            PolishError::ConsentRequired {
                host: "api.groq.com".to_string()
            }
        );

        // Consent to a different host must not cover this one, exactly as in
        // the rewrite guard.
        cfg.consented_hosts = vec!["api.deepseek.com".to_string()];
        assert_eq!(
            gate_for(&cfg).unwrap_err(),
            PolishError::ConsentRequired {
                host: "api.groq.com".to_string()
            }
        );
    }

    #[test]
    fn consented_hosted_probe_is_allowed() {
        let mut cfg = hosted_cfg();
        cfg.consented_hosts = vec!["api.groq.com".to_string()];
        assert_eq!(gate_for(&cfg), Ok(()));
    }

    #[test]
    fn a_consented_probe_runs_with_both_features_off() {
        // The connection is checked from the settings section before either
        // feature is switched on, so no switch gates the check.
        let mut cfg = hosted_cfg();
        cfg.rewrite.enabled = false;
        cfg.chat.enabled = false;
        cfg.consented_hosts = vec!["api.groq.com".to_string()];
        assert_eq!(gate_for(&cfg), Ok(()));
    }

    #[test]
    fn a_blocked_probe_reports_its_reason_and_the_consent_host() {
        let consent = PolishError::ConsentRequired {
            host: "api.groq.com".to_string(),
        };
        let status = blocked_probe_status(&consent, "api.groq.com");
        assert!(!status.reachable);
        assert_eq!(status.kind, "consent_required");
        // The consent key, so the line names what the user would be allowing.
        assert_eq!(status.detail, "api.groq.com");
        assert!(status.models.is_empty());
    }

    #[test]
    fn an_unconsented_hosted_list_is_refused() {
        // Gemini lists from its native path rather than from the base its chat
        // requests use, so the gate has two URLs to resolve on this row.
        let gemini = AiConfig {
            provider: "gemini".to_string(),
            ..base_cfg()
        };
        assert_eq!(
            list_gate(&gemini).unwrap_err(),
            ModelListError::ConsentRequired
        );

        let anthropic = AiConfig {
            provider: "anthropic".to_string(),
            ..base_cfg()
        };
        assert_eq!(
            list_gate(&anthropic).unwrap_err(),
            ModelListError::ConsentRequired
        );
    }

    #[test]
    fn a_consented_hosted_list_names_the_native_url() {
        let cfg = AiConfig {
            provider: "gemini".to_string(),
            consented_hosts: vec!["generativelanguage.googleapis.com".to_string()],
            ..base_cfg()
        };
        let targets = list_gate(&cfg).expect("a consented host passes");
        assert_eq!(
            targets.list_url,
            "https://generativelanguage.googleapis.com/v1beta/models"
        );
        assert!(targets.list.is_hosted);
    }

    #[test]
    fn a_local_list_needs_no_consent() {
        let cfg = base_cfg();
        assert!(cfg.consented_hosts.is_empty());
        let targets = list_gate(&cfg).expect("a local row reaches nobody");
        assert_eq!(targets.list_url, "http://localhost:11434/api/tags");
        assert!(!targets.list.is_hosted);
    }

    #[test]
    fn a_hand_typed_remote_http_list_is_refused_however_it_was_consented() {
        let mut cfg = custom_cfg("http://models.example.com/v1");
        cfg.consented_hosts = vec!["models.example.com".to_string()];
        assert_eq!(list_gate(&cfg).unwrap_err(), ModelListError::Unreachable);
    }

    #[test]
    fn a_refused_list_never_reads_the_key() {
        let reads = std::cell::Cell::new(0);
        let cfg = AiConfig {
            provider: "anthropic".to_string(),
            ..base_cfg()
        };

        let refused = gated_list_key(&cfg, |_| {
            reads.set(reads.get() + 1);
            Some("a key".to_string())
        });
        assert_eq!(refused.unwrap_err(), ModelListError::ConsentRequired);
        assert_eq!(
            reads.get(),
            0,
            "the keychain was read for a list that was never sent"
        );

        let consented = AiConfig {
            consented_hosts: vec!["api.anthropic.com".to_string()],
            ..cfg
        };
        let (targets, key) = gated_list_key(&consented, |provider| {
            reads.set(reads.get() + 1);
            Some(format!("key for {provider}"))
        })
        .expect("a consented host passes");
        assert_eq!(reads.get(), 1);
        assert_eq!(key.as_deref(), Some("key for anthropic"));
        assert_eq!(targets.list_url, "https://api.anthropic.com/v1/models");
    }

    #[test]
    fn a_local_list_reads_no_key_either() {
        let reads = std::cell::Cell::new(0);
        let (_, key) = gated_list_key(&base_cfg(), |_| {
            reads.set(reads.get() + 1);
            Some("a key".to_string())
        })
        .expect("a local row passes");
        assert_eq!(reads.get(), 0);
        assert_eq!(key, None);
    }

    #[test]
    fn the_list_and_probe_budgets_are_the_ones_the_record_names() {
        // ADR-040 section 3: the model list's timeout is 5 seconds.
        assert_eq!(MODEL_LIST_TIMEOUT, Duration::from_secs(5));
        // ADR-040 section 4: each knock on a local runtime gets 1 second.
        assert_eq!(LOCAL_PROBE_TIMEOUT, Duration::from_secs(1));
    }

    #[test]
    fn local_probe_is_never_gated() {
        let mut cfg = base_cfg();
        assert!(cfg.consented_hosts.is_empty());
        assert_eq!(gate_for(&cfg), Ok(()));
        cfg.rewrite.enabled = false;
        assert_eq!(gate_for(&cfg), Ok(()));
    }

    #[test]
    fn connection_reachable_lists_model() {
        let base = spawn_mock(
            "HTTP/1.1 200 OK",
            "Content-Type: application/json\r\nConnection: close\r\n",
            "{\"data\":[{\"id\":\"llama3\"},{\"id\":\"mistral\"}]}",
        );
        let status = check_against(&base, "llama3");
        assert!(status.reachable);
        assert_eq!(status.model_listed, Some(true));
        assert_eq!(status.kind, "ok");
        // Sorted, and the full list is exposed for the picker.
        assert_eq!(
            status.models,
            vec!["llama3".to_string(), "mistral".to_string()]
        );
    }

    #[test]
    fn connection_reachable_model_missing() {
        let base = spawn_mock(
            "HTTP/1.1 200 OK",
            "Content-Type: application/json\r\nConnection: close\r\n",
            "{\"data\":[{\"id\":\"mistral\"}]}",
        );
        let status = check_against(&base, "llama3");
        assert!(status.reachable);
        assert_eq!(status.model_listed, Some(false));
        assert_eq!(status.kind, "model_missing");
        assert_eq!(status.detail, "llama3");
    }

    #[test]
    fn connection_unauthorized() {
        let base = spawn_mock(
            "HTTP/1.1 401 Unauthorized",
            "Content-Length: 0\r\nConnection: close\r\n",
            "",
        );
        let status = check_against(&base, "llama3");
        assert!(status.reachable);
        assert_eq!(status.model_listed, None);
        assert_eq!(status.kind, "unauthorized");
        // The host, not the code: the list answers one refusal for 401 and 403
        // alike, and the row names what would not take the key.
        assert_eq!(status.detail, "127.0.0.1:0");
    }

    #[test]
    fn connection_refused_when_nothing_listens() {
        // Bind then drop to obtain a port with no listener.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let base = format!("http://127.0.0.1:{port}/v1");
        let status = check_against(&base, "llama3");
        assert!(!status.reachable);
        assert_eq!(status.kind, "refused");
    }

    #[test]
    fn a_body_that_is_not_a_model_list_is_a_malformed_answer() {
        let base = spawn_mock(
            "HTTP/1.1 200 OK",
            "Content-Type: application/json\r\nConnection: close\r\n",
            "not json",
        );
        let status = check_against(&base, "llama3");
        assert!(status.reachable);
        assert_eq!(status.kind, "error");
        assert!(status.models.is_empty());
    }

    #[test]
    fn a_transport_failure_is_read_as_one_of_two_things() {
        assert_eq!(
            list_error_for(TransportFailure::Timeout),
            ModelListError::Timeout
        );
        assert_eq!(
            list_error_for(TransportFailure::Connect),
            ModelListError::Unreachable
        );
        assert_eq!(
            list_error_for(TransportFailure::Other),
            ModelListError::Unreachable
        );
    }

    #[test]
    fn a_status_says_whether_the_key_or_the_server_was_the_trouble() {
        assert_eq!(list_error_for_status(200), None);
        assert_eq!(list_error_for_status(204), None);
        assert_eq!(
            list_error_for_status(401),
            Some(ModelListError::Unauthorized)
        );
        assert_eq!(
            list_error_for_status(403),
            Some(ModelListError::Unauthorized)
        );
        assert_eq!(
            list_error_for_status(500),
            Some(ModelListError::Status { code: 500 })
        );
        assert_eq!(
            list_error_for_status(404),
            Some(ModelListError::Status { code: 404 })
        );
    }

    /// One page of the Anthropic list, with the cursor that follows it.
    fn anthropic_page(ids: &[&str], has_more: bool) -> String {
        let rows: Vec<String> = ids
            .iter()
            .map(|id| format!("{{\"id\":\"{id}\",\"display_name\":\"{id}\"}}"))
            .collect();
        let last = ids.last().copied().unwrap_or_default();
        format!(
            "{{\"data\":[{}],\"has_more\":{has_more},\"last_id\":\"{last}\"}}",
            rows.join(",")
        )
    }

    /// Runs the driver over recorded pages, handing back what it read and the
    /// cursor each page was asked for.
    fn pages_of(bodies: Vec<String>) -> (Result<Vec<String>, ModelListError>, Vec<Option<String>>) {
        let served = Mutex::new((0usize, bodies, Vec::<Option<String>>::new()));
        let ids =
            tauri::async_runtime::block_on(collect_list_pages(ListFamily::Anthropic, |cursor| {
                let mut state = served.lock().expect("served");
                let (index, bodies, cursors) = &mut *state;
                cursors.push(cursor);
                let body = bodies
                    .get(*index)
                    .cloned()
                    .unwrap_or_else(|| anthropic_page(&[], false));
                *index += 1;
                std::future::ready(Ok(body))
            }));
        let cursors = served.into_inner().expect("served").2;
        (ids, cursors)
    }

    #[test]
    fn a_list_that_fits_on_one_page_is_read_once() {
        let (ids, cursors) = pages_of(vec![anthropic_page(&["claude-b", "claude-a"], false)]);
        assert_eq!(
            ids.expect("ids"),
            vec!["claude-a".to_string(), "claude-b".to_string()]
        );
        assert_eq!(cursors, vec![None]);
    }

    #[test]
    fn a_cursor_is_followed_to_the_end_of_the_list() {
        let (ids, cursors) = pages_of(vec![
            anthropic_page(&["a"], true),
            anthropic_page(&["b"], true),
            anthropic_page(&["c"], false),
        ]);
        assert_eq!(
            ids.expect("ids"),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        // Each page is asked for with the cursor the page before it answered,
        // which is the half of pagination a growing list depends on.
        assert_eq!(
            cursors,
            vec![None, Some("a".to_string()), Some("b".to_string())]
        );
    }

    #[test]
    fn a_page_that_is_not_a_list_ends_the_read() {
        let (ids, cursors) = pages_of(vec![
            anthropic_page(&["a"], true),
            "{\"nonsense\":true}".to_string(),
        ]);
        assert_eq!(ids.expect_err("stopped"), ModelListError::Malformed);
        assert_eq!(
            cursors.len(),
            2,
            "the read stopped where it could not parse"
        );
    }

    #[test]
    fn an_endless_cursor_stops_at_the_page_cap() {
        // Every page says there is another, which a provider answering its own
        // cursor forever would. The read ends and the picker still opens.
        let forever: Vec<String> = (0..MAX_LIST_PAGES + 10)
            .map(|n| anthropic_page(&[&format!("m{n:02}")], true))
            .collect();
        let (ids, cursors) = pages_of(forever);
        assert_eq!(ids.expect("ids").len(), MAX_LIST_PAGES);
        assert_eq!(cursors.len(), MAX_LIST_PAGES);
    }

    #[test]
    fn a_page_url_carries_the_cursor_its_family_asks_for() {
        let anthropic = list_page_url(
            ListFamily::Anthropic,
            "https://api.anthropic.com/v1/models",
            Some("sk-ant"),
            Some("model-42"),
        )
        .expect("url");
        assert!(anthropic.contains("limit=1000"), "{anthropic}");
        assert!(anthropic.contains("after_id=model-42"), "{anthropic}");
        // The key rides a header on this wire, never the query.
        assert!(!anthropic.contains("sk-ant"), "{anthropic}");

        let openai = list_page_url(
            ListFamily::OpenAi,
            "https://api.openai.com/v1/models",
            Some("sk-openai"),
            None,
        )
        .expect("url");
        assert_eq!(openai, "https://api.openai.com/v1/models");
    }

    #[test]
    fn a_gemini_failure_names_neither_the_query_nor_the_key() {
        const KEY: &str = "ZZ-gemini-key-that-must-never-be-logged";
        let url = list_page_url(
            ListFamily::Gemini,
            "https://generativelanguage.googleapis.com/v1beta/models",
            Some(KEY),
            Some("page-2"),
        )
        .expect("url");
        assert!(url.contains("pageSize=1000"), "{url}");
        assert!(url.contains("pageToken=page-2"), "{url}");
        assert!(url.contains(KEY), "the key rides the query on this wire");

        // Everything the failure of that request can be, as the frontend and
        // the log see it.
        for error in [
            list_error_for(TransportFailure::Timeout),
            list_error_for(TransportFailure::Connect),
            ModelListError::Unauthorized,
            ModelListError::Malformed,
            ModelListError::Status { code: 500 },
        ] {
            let sentence = error.to_string();
            assert!(!sentence.contains(KEY), "{sentence}");
            assert!(!sentence.contains("key="), "{sentence}");
            assert!(!sentence.contains("http"), "{sentence}");
            assert!(!list_error_kind(&error).contains(KEY));
        }
        // And the one place a raw client error could carry it.
        assert!(
            !sanitize_ai_error(&format!("error sending request for url ({url})")).contains(KEY)
        );
    }

    #[test]
    fn a_gemini_body_answers_its_own_cursor() {
        assert_eq!(
            next_page_token("{\"models\":[],\"nextPageToken\":\"abc\"}"),
            Some("abc".to_string())
        );
        assert_eq!(next_page_token("{\"models\":[]}"), None);
        assert_eq!(next_page_token("{\"nextPageToken\":\"\"}"), None);
        assert_eq!(next_page_token("not json"), None);
    }

    #[test]
    fn every_command_here_is_in_the_invoke_handler() {
        // A command that is not registered cannot be called however well it
        // behaves, and the settings section calls all of these on open.
        const LIB_RS: &str = include_str!("../lib.rs");
        for command in [
            "commands::ai::ai_providers",
            "commands::ai::ai_list_models",
            "commands::ai::ai_probe_local",
            "commands::ai::ai_check_connection",
            "commands::ai::ai_endpoint_state",
            "commands::ai::ai_consent_host",
            "commands::ai::ai_set_api_key",
            "commands::ai::ai_clear_api_key",
            "commands::ai::ai_has_api_key",
        ] {
            assert!(LIB_RS.contains(command), "{command} is not registered");
        }
    }

    #[test]
    fn the_table_is_answered_whole() {
        let rows = ai_providers();
        assert_eq!(rows.len(), writ_core::ai::providers::PROVIDERS.len());
        assert_eq!(rows[0].id, "ollama");
        let json = serde_json::to_string(&rows).expect("serialised");
        assert!(json.contains("\"group\":\"local\""), "{json}");
        assert!(json.contains("\"wire\":\"anthropic\""), "{json}");
    }

    /// A socket that answers one request with `status_line` and a tiny body,
    /// handing back the raw request bytes it read.
    fn spawn_knock_target(status_line: &'static str) -> (String, Arc<Mutex<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let seen = Arc::new(Mutex::new(String::new()));
        let recorder = seen.clone();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                if let Ok(read) = stream.read(&mut buf) {
                    *recorder.lock().expect("recorder") =
                        String::from_utf8_lossy(&buf[..read]).to_string();
                }
                let _ = stream.write_all(
                    format!("{status_line}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}")
                        .as_bytes(),
                );
                let _ = stream.flush();
            }
        });
        (format!("http://127.0.0.1:{}/api/tags", addr.port()), seen)
    }

    /// An address with nothing behind it.
    fn dead_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);
        format!("http://127.0.0.1:{port}/v1/models")
    }

    #[test]
    fn a_runtime_that_answers_is_running_and_one_that_does_not_is_not() {
        let (alive, seen) = spawn_knock_target("HTTP/1.1 200 OK");
        let probe = tauri::async_runtime::block_on(probe_local_at(&alive, &dead_url()));
        assert!(probe.ollama, "a 200 on the port is a running runtime");
        assert!(!probe.lmstudio, "nothing listens on that port");

        // The knock carries the request line and no credential of any kind.
        let request = seen.lock().expect("seen").clone();
        assert!(request.starts_with("GET /api/tags HTTP/1.1"), "{request}");
        // The knock is the request line and what the client will not leave
        // out: an empty user-agent, `accept`, and the host it is dialling.
        let headers: Vec<String> = request
            .lines()
            .skip(1)
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.split(':').next().unwrap_or_default().to_lowercase())
            .collect();
        assert_eq!(headers, vec!["accept", "user-agent", "host"], "{request}");
        assert!(request.contains("user-agent: \r\n"), "{request}");
        assert!(
            !request.to_lowercase().contains("authorization"),
            "{request}"
        );
        assert!(!request.to_lowercase().contains("cookie"), "{request}");
        assert!(!request.to_lowercase().contains("x-api-key"), "{request}");
    }

    #[test]
    fn a_runtime_that_answers_an_error_is_not_running() {
        let (refusing, _seen) = spawn_knock_target("HTTP/1.1 500 Internal Server Error");
        let probe = tauri::async_runtime::block_on(probe_local_at(&refusing, &dead_url()));
        assert!(!probe.ollama);
    }

    #[test]
    fn model_listed_among_handles_empty_and_missing() {
        let ids = vec!["a".to_string()];
        assert_eq!(model_listed_among(&ids, ""), None);
        assert_eq!(model_listed_among(&[], "a"), None);
        assert_eq!(model_listed_among(&ids, "a"), Some(true));
        assert_eq!(model_listed_among(&ids, "b"), Some(false));
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
