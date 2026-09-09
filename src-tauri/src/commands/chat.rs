//! IPC surface for the chat pane.
//!
//! The *policy* — which providers exist, what a request body looks like, what
//! one line of a stream means, and what a reply's proposal is — lives in
//! [`writ_core::chat`]. This module is the *mechanism*: it reads the attached
//! notes, resolves the endpoint and the key, streams the reply to the frontend
//! as `writ://ai-chat`, and writes a proposal the user applied through the one
//! guarded facade.
//!
//! What it does not do is add a second anything. The keychain, the HTTP
//! client, the timeouts, the URL redaction and the host-consent record are
//! [`super::ai`]'s and are reused here (ADR-031 rules 2.6 and 6.1), and the
//! only writer of a note file is
//! [`writ_storage::guarded::write_note_guarded`].
//!
//! Privacy invariants enforced here:
//! - The request is assembled from the notes named in the call and nothing
//!   else. No folder is swept and no neighbour is added (ADR-031 rule 2.5).
//! - A path outside the notes folder is refused before it is opened, by the
//!   same containment resolution the rest of the app writes through.
//! - The endpoint guard and the consent check run before the key is read, so a
//!   hand-edited `config.toml` reaches no host and raises no keychain prompt.
//! - Nothing logged here can hold a prompt, a reply or a note: the `tracing`
//!   lines carry lengths, a host, a model id and a status code, and the
//!   activity record has no field text could go in (ADR-031 rules 5.1, 5.2).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use writ_core::activity::{ActivityRecord, Actor, Decision};
use writ_core::chat::{self, AttachedNote, ChatError, ChatTurn, Delta, Proposal, Provider};
use writ_core::config::AiConfig;
use writ_core::hash::digest_from_hex;
use writ_core::notes::guard::DiskState;
use writ_core::notes::WriteOrigin;
use writ_core::polish;
use writ_storage::guarded::{write_note_guarded, ConflictPolicy, DiskRead, GuardedWrite};

use super::ai::AiKeyState;
use crate::events::{emit_event, WritFrontendEvent};
use crate::poison::recover_poison;
use crate::state::AppState;

/// The largest note the pane will attach. A note past this is refused by name
/// rather than truncated: half a note read as the whole one is what a proposal
/// would then be built from.
const MAX_ATTACHED_BYTES: u64 = 2 * 1024 * 1024;

/// How many notes one request may carry, so a loop in a caller cannot assemble
/// an unbounded body.
const MAX_ATTACHED_NOTES: usize = 20;

/// Session-scoped state for the pane: the cancel flag of each live stream,
/// keyed by the conversation the frontend named.
#[derive(Default)]
pub struct ChatState {
    tasks: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl ChatState {
    /// Registers a conversation and hands back its cancel flag.
    ///
    /// Called before the request task is spawned, so a cancel that races it
    /// cannot miss the flag and the task's own cleanup has an entry to remove.
    pub fn begin(&self, conversation_id: &str) -> Arc<AtomicBool> {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut tasks = recover_poison(self.tasks.lock(), "commands::chat::begin");
        tasks.insert(conversation_id.to_string(), cancel.clone());
        cancel
    }

    /// Raises a conversation's cancel flag. `false` when nothing is live under
    /// that id, which is what a cancel arriving after a reply finished is.
    pub fn cancel(&self, conversation_id: &str) -> bool {
        let tasks = recover_poison(self.tasks.lock(), "commands::chat::cancel");
        match tasks.get(conversation_id) {
            Some(cancel) => {
                cancel.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// Forgets a conversation whose request has ended.
    pub fn finish(&self, conversation_id: &str) {
        let mut tasks = recover_poison(self.tasks.lock(), "commands::chat::finish");
        tasks.remove(conversation_id);
    }

    /// How many conversations are live.
    pub fn live(&self) -> usize {
        recover_poison(self.tasks.lock(), "commands::chat::live").len()
    }
}

/// Where the chat endpoint points and what it still needs.
///
/// Mirrors [`super::ai::AiEndpointState`] rather than sharing it: the two
/// surfaces have separate endpoints, and one struct for both would let a
/// reading of one be rendered for the other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatEndpointState {
    /// Whether `[ai.chat] enabled` is on.
    pub enabled: bool,
    /// The configured provider id, whether or not this build speaks it.
    pub provider: String,
    /// The configured model id.
    pub model: String,
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
    /// Whether a key is stored for this provider, and where it lives.
    pub key_state: AiKeyState,
}

/// One turn as the frontend sends it.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatTurnDto {
    /// `user` or `assistant`.
    pub role: String,
    /// What was said.
    pub content: String,
}

/// What a proposal the user applied did to the note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProposalOutcome {
    /// The note, folder-relative, as the proposal named it.
    pub path: String,
    /// What the note holds now, hex-encoded.
    pub hash: String,
    /// How many bytes were written.
    pub bytes: u64,
}

/// Builds the endpoint state for `cfg`. Pure over its key lookup, so the
/// consent/key matrix is testable without a keychain.
pub fn endpoint_state_from(cfg: &AiConfig, key_state: AiKeyState) -> ChatEndpointState {
    let base = ChatEndpointState {
        enabled: cfg.chat.enabled,
        provider: cfg.chat.provider.clone(),
        model: cfg.chat.model.clone(),
        host: None,
        host_port: None,
        is_hosted: false,
        is_allowed: false,
        is_consented: false,
        key_state,
    };
    match polish::resolve_endpoint(&cfg.chat.base_url) {
        Ok(target) => ChatEndpointState {
            is_consented: !target.is_hosted || super::ai::is_consented(cfg, &target.host),
            host: Some(target.host),
            host_port: Some(target.host_port),
            is_hosted: target.is_hosted,
            is_allowed: target.is_allowed,
            ..base
        },
        Err(_) => base,
    }
}

/// Whether answering "is a key set?" would reach the keychain for nothing.
///
/// A local endpoint needs no key, and every keychain read on macOS can raise a
/// system password prompt, so the question is not asked for a local one.
pub fn needs_key_lookup(cfg: &AiConfig) -> bool {
    match polish::resolve_endpoint(&cfg.chat.base_url) {
        Ok(target) => target.is_hosted,
        Err(_) => false,
    }
}

/// The account a key for the configured provider is stored under, or `None`
/// when the provider id is not one this build speaks.
pub fn key_account(cfg: &AiConfig) -> Option<&'static str> {
    Provider::parse(&cfg.chat.provider)
        .ok()
        .map(Provider::key_account)
}

fn chat_config(app: &AppHandle) -> AiConfig {
    let state = app.state::<AppState>();
    let guard = recover_poison(state.config.lock(), "commands::chat::config");
    guard.ai.clone()
}

/// Reports where the chat endpoint points and what it still needs, so the pane
/// can ask for consent or a key before a note is sent rather than after.
#[tauri::command]
pub fn chat_state(app: AppHandle) -> ChatEndpointState {
    let cfg = chat_config(&app);
    let key_state = match (needs_key_lookup(&cfg), key_account(&cfg)) {
        (true, Some(account)) => super::ai::key_state_for(&app, account),
        _ => AiKeyState {
            is_set: false,
            memory_only: false,
        },
    };
    endpoint_state_from(&cfg, key_state)
}

/// Everything a stream needs, resolved from config and validated.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedChat {
    /// Where the request goes.
    pub endpoint: String,
    /// Which wire format it speaks.
    pub provider: Provider,
    /// The body, built by [`writ_core::chat::build_request_body`].
    pub body: serde_json::Value,
    /// The key, for a hosted endpoint that has one.
    pub api_key: Option<String>,
    /// The host, for the log line and the activity record.
    pub host: String,
    /// The endpoint is on this machine.
    pub is_localhost: bool,
    /// The notes the request carries, and nothing else.
    pub context: Vec<AttachedNote>,
}

/// Validates config and inputs and resolves the request.
///
/// `lookup_key` maps a keychain account to its key and is consulted only for a
/// hosted endpoint, after consent, so a refused request reads no credential.
pub fn prepare_chat(
    cfg: &AiConfig,
    turns: &[ChatTurn],
    context: Vec<AttachedNote>,
    lookup_key: impl FnOnce(&str) -> Option<String>,
) -> Result<PreparedChat, ChatError> {
    if !cfg.chat.enabled {
        return Err(ChatError::Disabled);
    }
    let provider = Provider::parse(&cfg.chat.provider)?;
    if turns
        .iter()
        .last()
        .is_none_or(|turn| turn.content.trim().is_empty())
    {
        return Err(ChatError::EmptyMessage);
    }

    // The one authority: the guard here and `ai_consent_host` resolve the host
    // through the same call, so the string checked is the string recorded.
    let target = polish::resolve_endpoint(&cfg.chat.base_url)?;
    if !target.is_allowed {
        return Err(ChatError::EndpointNotAllowed);
    }
    if cfg.chat.model.trim().is_empty() {
        return Err(ChatError::ModelRequired);
    }

    let api_key = if target.is_hosted {
        if !super::ai::is_consented(cfg, &target.host) {
            return Err(ChatError::ConsentRequired {
                host: target.host.clone(),
            });
        }
        match lookup_key(provider.key_account()) {
            Some(key) => Some(key),
            None => {
                return Err(ChatError::ApiKeyRequired {
                    host: target.host.clone(),
                })
            }
        }
    } else {
        None
    };

    let body = chat::build_request_body(
        provider,
        &cfg.chat.model,
        chat::SYSTEM_PROMPT,
        turns,
        &context,
    );
    Ok(PreparedChat {
        endpoint: chat::endpoint(provider, &cfg.chat.base_url),
        provider,
        body,
        api_key,
        is_localhost: !target.is_hosted,
        host: target.host,
        context,
    })
}

/// The note file a folder-relative (or absolute) path names.
///
/// Every existing part of the path is canonicalised before the containment
/// check, so neither the file nor a linked directory above it can carry an
/// answer out of the notes folder.
pub fn note_file_in(notes_root: &Path, path: &str) -> Result<PathBuf, String> {
    let given = Path::new(path);
    let candidate = match given.is_absolute() {
        true => given.to_path_buf(),
        false => notes_root.join(given),
    };
    let Some(resolved) = crate::security::resolve_for_containment(&candidate) else {
        return Err(outside_notes(path));
    };
    if !writ_core::notes::containment::is_inside(notes_root, Path::new(&resolved)) {
        return Err(outside_notes(path));
    }
    let file = PathBuf::from(resolved);
    if !file.is_file() {
        return Err(format!("{path} is not a note."));
    }
    Ok(file)
}

fn outside_notes(path: &str) -> String {
    format!("{path} is not in the notes folder.")
}

/// The note's path as the pane lists it and a proposal names it: relative to
/// the notes folder, so nothing that leaves the machine carries the absolute
/// path of the folder it came from.
fn relative_key(notes_root: &Path, file: &Path) -> String {
    match file.strip_prefix(notes_root) {
        Ok(rest) => rest.to_string_lossy().into_owned(),
        Err(_) => file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

/// Reads the notes the call named, in the order it named them.
///
/// This is the whole of what the request carries. Nothing walks the folder and
/// nothing consults the index (ADR-031 rule 2.5).
pub fn read_attached_in(notes_root: &Path, paths: &[String]) -> Result<Vec<AttachedNote>, String> {
    if paths.len() > MAX_ATTACHED_NOTES {
        return Err(format!(
            "Attach at most {MAX_ATTACHED_NOTES} notes to one conversation."
        ));
    }
    let mut notes: Vec<AttachedNote> = Vec::with_capacity(paths.len());
    for path in paths {
        let file = note_file_in(notes_root, path)?;
        let key = relative_key(notes_root, &file);
        if notes.iter().any(|note| note.path == key) {
            continue;
        }
        let size = std::fs::metadata(&file)
            .map(|m| m.len())
            .unwrap_or_default();
        if size > MAX_ATTACHED_BYTES {
            return Err(format!("{key} is too large to attach."));
        }
        let bytes = std::fs::read(&file).map_err(|_| format!("{key} could not be read."))?;
        let text = String::from_utf8(bytes).map_err(|_| format!("{key} is not text."))?;
        notes.push(AttachedNote {
            before_hash: writ_core::hash::sha256_hex(text.as_bytes()),
            path: key,
            text,
        });
    }
    Ok(notes)
}

/// Writes one proposal and records what became of it.
///
/// The guard is [`write_note_guarded`]'s, with the proposal's `before_hash` as
/// what Writ last saw the note hold: a note changed since the proposal was
/// made is refused, and the refusal writes the proposed text beside it as a
/// dated copy rather than over it (ADR-031 rule 4.3). The write carries no
/// ignore stamp, so a tab holding the note learns about it through the folder
/// watcher the way it learns about any other write it did not make (ADR-033).
pub fn apply_proposal_inner(
    notes_root: &Path,
    writ_dir: &Path,
    host: &str,
    path: &str,
    new_content: &str,
    before_hash: &str,
) -> Result<ProposalOutcome, String> {
    let file = note_file_in(notes_root, path)?;
    let key = relative_key(notes_root, &file);
    let Some(digest) = digest_from_hex(before_hash) else {
        return Err(format!("The recorded state of {key} is not readable."));
    };

    let outcome = write_note_guarded(
        GuardedWrite {
            target: &file,
            bytes: new_content.as_bytes(),
            // Only the digest is read out of this: `decide_save` compares
            // digests, never the length or the modification time, neither of
            // which the pane knew about the text it showed.
            last_known: Some(DiskState {
                hash: digest,
                size: 0,
                mtime: None,
            }),
            on_disk: DiskRead::Fresh,
            dataless: None,
            origin: WriteOrigin::Chat,
            on_conflict: ConflictPolicy::RefuseWithCopy,
            history: None,
        },
        None,
    );

    match outcome {
        Ok(written) => {
            record_proposal(
                writ_dir,
                host,
                "apply_proposal",
                &key,
                Decision::Allow,
                Some(written.disk_state.size),
            );
            Ok(ProposalOutcome {
                path: key,
                hash: writ_core::hash::digest_hex(written.disk_state.hash),
                bytes: written.disk_state.size,
            })
        }
        Err(error) => {
            record_proposal(
                writ_dir,
                host,
                "apply_proposal",
                &key,
                Decision::Refuse,
                None,
            );
            Err(error.to_string())
        }
    }
}

/// Records that a proposal was read and not applied.
///
/// Every proposal reaches the log, applied or not, so the record of what a
/// model asked for does not depend on the answer (ADR-031 rule 5.5). A path
/// the folder does not hold is still recorded, under the name it was given.
pub fn discard_proposal_inner(notes_root: &Path, writ_dir: &Path, host: &str, path: &str) {
    let key = note_file_in(notes_root, path)
        .map(|file| relative_key(notes_root, &file))
        .unwrap_or_else(|_| path.to_string());
    record_proposal(
        writ_dir,
        host,
        "discard_proposal",
        &key,
        Decision::Refuse,
        None,
    );
}

/// Appends one line about a proposal.
///
/// A log that cannot be written is not fatal: the write it describes already
/// happened, and losing the line does not undo it.
fn record_proposal(
    writ_dir: &Path,
    host: &str,
    action: &str,
    key: &str,
    decision: Decision,
    bytes: Option<u64>,
) {
    let mut record = ActivityRecord::now(
        Actor::Chat {
            host: host.to_string(),
        },
        action,
        decision,
    )
    .with_path(key);
    if let Some(bytes) = bytes {
        record = record.with_bytes(bytes);
    }
    if let Err(error) = writ_storage::activity_log::append(writ_dir, &record) {
        tracing::warn!(error = %error, "the activity log did not take a chat record");
    }
}

fn emit_chat(app: &AppHandle, conversation_id: &str, kind: &str, text: Option<String>) {
    emit_chat_with(app, conversation_id, kind, text, Vec::new());
}

fn emit_chat_with(
    app: &AppHandle,
    conversation_id: &str,
    kind: &str,
    text: Option<String>,
    proposals: Vec<Proposal>,
) {
    if let Err(error) = emit_event(
        app,
        WritFrontendEvent::AiChat {
            conversation_id: conversation_id.to_string(),
            kind: kind.to_string(),
            text,
            proposals,
        },
    ) {
        tracing::warn!(error = %error, "failed to emit ai-chat event");
    }
}

/// The one line a send writes to the log.
///
/// Every field here is a count, a host or a model id. There is no field for
/// the prompt, the reply or the note, which is what makes ADR-031 rule 5.2
/// checkable rather than a habit.
fn log_request(prepared: &PreparedChat, note_bytes: usize, turns: usize) {
    tracing::info!(
        host = %prepared.host,
        provider = prepared.provider.as_str(),
        notes = prepared.context.len(),
        note_bytes,
        turns,
        "sending a chat request"
    );
}

/// The one line a refused request writes. The status code is the whole of what
/// the server said that may be recorded; its body is never read.
fn log_rejected(status: u16) {
    tracing::warn!(status, "chat request rejected");
}

/// One thing that happens during a stream.
enum ChatEvent {
    Chunk(String),
    Done,
    Error(String),
}

/// Sends the request and streams the reply, invoking `on_event` for each
/// delta, the terminal `Done`, or an `Error`. Stops early when `cancel` is
/// set, emitting nothing further.
async fn run_chat_stream(
    client: &reqwest::Client,
    prepared: &PreparedChat,
    cancel: &AtomicBool,
    mut on_event: impl FnMut(ChatEvent),
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
        Ok(response) => response,
        Err(error) => {
            on_event(ChatEvent::Error(super::ai::connection_error_message(
                &error,
                prepared.is_localhost,
            )));
            return;
        }
    };

    let status = response.status();
    if !status.is_success() {
        log_rejected(status.as_u16());
        on_event(ChatEvent::Error(format!(
            "The model server returned status {}.",
            status.as_u16()
        )));
        return;
    }

    let mut stream = response.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(item) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let bytes = match item {
            Ok(bytes) => bytes,
            Err(error) => {
                on_event(ChatEvent::Error(super::ai::sanitize_ai_error(
                    &error.to_string(),
                )));
                return;
            }
        };
        buf.extend_from_slice(&bytes);
        for line in super::ai::drain_complete_lines(&mut buf) {
            match chat::parse_delta(prepared.provider, &line) {
                Delta::Text(text) => on_event(ChatEvent::Chunk(text)),
                Delta::Done => {
                    on_event(ChatEvent::Done);
                    return;
                }
                // The category is the whole of what the provider said that may
                // be shown: the message beside it can quote the request.
                Delta::Failed(kind) => {
                    tracing::warn!(kind = %kind, "the model server ended the stream");
                    on_event(ChatEvent::Error(
                        "The model server ended the reply.".to_string(),
                    ));
                    return;
                }
                Delta::Ignore => {}
            }
        }
    }

    if cancel.load(Ordering::Relaxed) {
        return;
    }
    on_event(ChatEvent::Done);
}

/// Starts a streaming reply. The frontend supplies `conversation_id` so it can
/// match `writ://ai-chat` events (and cancel) with no window in which an early
/// event could arrive unmatched. Validation runs synchronously; the network
/// work is spawned.
#[tauri::command]
pub async fn chat_send(
    app: AppHandle,
    conversation_id: String,
    turns: Vec<ChatTurnDto>,
    context_paths: Vec<String>,
) -> Result<String, String> {
    let turns: Vec<ChatTurn> = turns.into_iter().map(ChatTurn::from).collect();
    let cfg = chat_config(&app);

    let context = {
        let notes_root = app.state::<AppState>().notes_root();
        read_attached_in(&notes_root, &context_paths)?
    };
    let attached_bytes: usize = context.iter().map(|note| note.text.len()).sum();

    let prepared = prepare_chat(&cfg, &turns, context, |account| {
        super::ai::key_for(&app, account)
    })
    .map_err(|error| error.to_string())?;

    log_request(&prepared, attached_bytes, turns.len());

    let client = super::ai::build_client()?;
    let cancel = app.state::<ChatState>().begin(&conversation_id);

    let task_app = app.clone();
    let task_id = conversation_id.clone();
    tauri::async_runtime::spawn(async move {
        let mut reply = String::new();
        run_chat_stream(&client, &prepared, &cancel, |event| match event {
            ChatEvent::Chunk(text) => {
                reply.push_str(&text);
                emit_chat(&task_app, &task_id, "chunk", Some(text));
            }
            ChatEvent::Done => {
                let proposals = chat::parse_proposals(&reply, &prepared.context);
                emit_chat_with(&task_app, &task_id, "done", None, proposals);
            }
            ChatEvent::Error(message) => emit_chat(&task_app, &task_id, "error", Some(message)),
        })
        .await;

        task_app.state::<ChatState>().finish(&task_id);
    });

    Ok(conversation_id)
}

/// Signals a live reply to stop. Further deltas are dropped and no terminal
/// event is emitted, so the text already on screen stays and no proposal is
/// read out of half a reply.
#[tauri::command]
pub fn chat_cancel(chat: State<'_, ChatState>, conversation_id: String) {
    chat.cancel(&conversation_id);
}

/// Writes a proposal the user applied, and records what became of it.
#[tauri::command]
pub fn chat_apply_proposal(
    app: AppHandle,
    path: String,
    new_content: String,
    before_hash: String,
) -> Result<ProposalOutcome, String> {
    let (notes_root, writ_dir) = {
        let state = app.state::<AppState>();
        (state.notes_root(), state.writ_dir.clone())
    };
    let outcome = apply_proposal_inner(
        &notes_root,
        &writ_dir,
        &chat_host(&app),
        &path,
        &new_content,
        &before_hash,
    );
    announce_activity(&app);
    outcome
}

/// Records that a proposal was read and not applied.
#[tauri::command]
pub fn chat_discard_proposal(app: AppHandle, path: String) {
    let (notes_root, writ_dir) = {
        let state = app.state::<AppState>();
        (state.notes_root(), state.writ_dir.clone())
    };
    discard_proposal_inner(&notes_root, &writ_dir, &chat_host(&app), &path);
    announce_activity(&app);
}

/// The host the pane is talking to, which is how the log names it.
fn chat_host(app: &AppHandle) -> String {
    let cfg = chat_config(app);
    polish::resolve_endpoint(&cfg.chat.base_url)
        .map(|target| target.host)
        .unwrap_or_default()
}

/// Tells any open activity view that the log grew.
fn announce_activity(app: &AppHandle) {
    if let Err(error) = emit_event(app, WritFrontendEvent::ActivityChanged {}) {
        tracing::warn!(error = %error, "failed to emit activity event");
    }
}

impl From<ChatTurnDto> for ChatTurn {
    fn from(dto: ChatTurnDto) -> Self {
        Self {
            role: match dto.role.as_str() {
                "assistant" => writ_core::chat::Role::Assistant,
                _ => writ_core::chat::Role::User,
            },
            content: dto.content,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(enabled: bool, base_url: &str, model: &str) -> AiConfig {
        AiConfig {
            chat: writ_core::config::AiChatConfig {
                enabled,
                provider: "openai_compatible".to_string(),
                base_url: base_url.to_string(),
                model: model.to_string(),
            },
            ..AiConfig::default()
        }
    }

    fn turns() -> Vec<ChatTurn> {
        vec![ChatTurn {
            role: writ_core::chat::Role::User,
            content: "What does this argue?".to_string(),
        }]
    }

    fn no_key(_account: &str) -> Option<String> {
        None
    }

    #[test]
    fn a_switch_that_is_off_refuses_before_anything_is_read() {
        let cfg = config(false, "http://localhost:11434/v1", "llama3");
        assert_eq!(
            prepare_chat(&cfg, &turns(), Vec::new(), no_key),
            Err(ChatError::Disabled)
        );
    }

    #[test]
    fn an_unconsented_hosted_host_is_refused_before_a_body_is_built() {
        let cfg = config(true, "https://api.example.com/v1", "some-model");
        assert_eq!(
            prepare_chat(&cfg, &turns(), Vec::new(), |_| panic!(
                "the key was read for a host with no consent"
            )),
            Err(ChatError::ConsentRequired {
                host: "api.example.com".to_string()
            })
        );
    }

    #[test]
    fn a_consented_hosted_host_still_needs_a_key() {
        let mut cfg = config(true, "https://api.example.com/v1", "some-model");
        cfg.consented_hosts = vec!["api.example.com".to_string()];
        assert_eq!(
            prepare_chat(&cfg, &turns(), Vec::new(), no_key),
            Err(ChatError::ApiKeyRequired {
                host: "api.example.com".to_string()
            })
        );
    }

    #[test]
    fn http_to_a_remote_host_is_refused() {
        let mut cfg = config(true, "http://api.example.com/v1", "some-model");
        cfg.consented_hosts = vec!["api.example.com".to_string()];
        assert_eq!(
            prepare_chat(&cfg, &turns(), Vec::new(), no_key),
            Err(ChatError::EndpointNotAllowed)
        );
    }

    #[test]
    fn a_local_endpoint_needs_neither_consent_nor_a_key() {
        let cfg = config(true, "http://localhost:11434/v1", "llama3");
        let prepared = prepare_chat(&cfg, &turns(), Vec::new(), |_| {
            panic!("a local endpoint read a key")
        })
        .expect("prepared");
        assert_eq!(
            prepared.endpoint,
            "http://localhost:11434/v1/chat/completions"
        );
        assert!(prepared.api_key.is_none());
        assert!(prepared.is_localhost);
    }

    #[test]
    fn a_model_nobody_chose_is_refused() {
        let cfg = config(true, "http://localhost:11434/v1", "  ");
        assert_eq!(
            prepare_chat(&cfg, &turns(), Vec::new(), no_key),
            Err(ChatError::ModelRequired)
        );
    }

    #[test]
    fn an_empty_message_sends_nothing() {
        let cfg = config(true, "http://localhost:11434/v1", "llama3");
        let blank = vec![ChatTurn {
            role: writ_core::chat::Role::User,
            content: "   ".to_string(),
        }];
        assert_eq!(
            prepare_chat(&cfg, &blank, Vec::new(), no_key),
            Err(ChatError::EmptyMessage)
        );
    }

    #[test]
    fn a_provider_this_build_does_not_speak_is_refused() {
        let mut cfg = config(true, "http://localhost:11434/v1", "llama3");
        cfg.chat.provider = "telepathy".to_string();
        assert_eq!(
            prepare_chat(&cfg, &turns(), Vec::new(), no_key),
            Err(ChatError::UnknownProvider("telepathy".to_string()))
        );
    }

    #[test]
    fn the_state_of_an_unparseable_url_names_no_host() {
        let cfg = config(true, "not a url", "llama3");
        let state = endpoint_state_from(
            &cfg,
            AiKeyState {
                is_set: false,
                memory_only: false,
            },
        );
        assert!(state.enabled);
        assert!(state.host.is_none());
        assert!(!state.is_allowed);
        assert!(!state.is_consented);
    }

    #[test]
    fn a_local_endpoint_reads_as_consented_and_raises_no_keychain_prompt() {
        let cfg = config(true, "http://localhost:11434/v1", "llama3");
        assert!(!needs_key_lookup(&cfg));
        let state = endpoint_state_from(
            &cfg,
            AiKeyState {
                is_set: false,
                memory_only: false,
            },
        );
        assert!(state.is_consented);
        assert!(!state.is_hosted);
    }

    #[test]
    fn each_provider_names_its_own_keychain_account() {
        let mut cfg = config(true, "https://api.anthropic.com", "claude-opus-5");
        cfg.chat.provider = "anthropic".to_string();
        assert_eq!(key_account(&cfg), Some("anthropic"));
        cfg.chat.provider = "openai_compatible".to_string();
        assert_eq!(key_account(&cfg), Some("openai_compatible"));
        cfg.chat.provider = "telepathy".to_string();
        assert_eq!(key_account(&cfg), None);
    }
}

#[cfg(test)]
mod stream_tests {
    use super::tests_support::*;
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn run_against(prepared: &PreparedChat, cancel: Arc<AtomicBool>) -> Vec<String> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        tauri::async_runtime::block_on(async move {
            let client = super::super::ai::build_client().expect("client");
            run_chat_stream(&client, prepared, &cancel, |event| {
                let mut seen = sink.lock().expect("events");
                match event {
                    ChatEvent::Chunk(text) => seen.push(format!("chunk:{text}")),
                    ChatEvent::Done => seen.push("done".to_string()),
                    ChatEvent::Error(message) => seen.push(format!("error:{message}")),
                }
            })
            .await;
        });
        Arc::try_unwrap(events)
            .expect("one reference")
            .into_inner()
            .expect("events")
    }

    #[test]
    fn a_recorded_reply_arrives_as_chunks_and_one_done() {
        let (base, _seen) = spawn_mock(
            "HTTP/1.1 200 OK",
            "Content-Type: text/event-stream\r\nConnection: close\r\n",
            ANTHROPIC_STREAM,
        );
        let prepared = prepared_for(&base, Provider::Anthropic, Some("secret-key"));
        let events = run_against(&prepared, Arc::new(AtomicBool::new(false)));
        let text: String = events
            .iter()
            .filter_map(|event| event.strip_prefix("chunk:"))
            .collect();
        assert_eq!(text, RECORDED_REPLY);
        assert_eq!(events.last().map(String::as_str), Some("done"));
    }

    #[test]
    fn the_messages_api_request_carries_the_key_in_its_own_header() {
        let (base, seen) = spawn_mock(
            "HTTP/1.1 200 OK",
            "Content-Type: text/event-stream\r\nConnection: close\r\n",
            ANTHROPIC_STREAM,
        );
        let prepared = prepared_for(&base, Provider::Anthropic, Some("secret-key"));
        run_against(&prepared, Arc::new(AtomicBool::new(false)));
        let request = seen.lock().expect("request").clone();
        assert!(request.contains("x-api-key: secret-key"), "got: {request}");
        assert!(
            request.contains("anthropic-version: 2023-06-01"),
            "got: {request}"
        );
        assert!(
            !request.to_lowercase().contains("authorization:"),
            "got: {request}"
        );
    }

    #[test]
    fn a_cancelled_stream_stops_and_says_nothing_further() {
        let (base, _seen) = spawn_mock(
            "HTTP/1.1 200 OK",
            "Content-Type: text/event-stream\r\nConnection: close\r\n",
            ANTHROPIC_STREAM,
        );
        let prepared = prepared_for(&base, Provider::Anthropic, None);
        let events = run_against(&prepared, Arc::new(AtomicBool::new(true)));
        assert!(events.is_empty(), "got: {events:?}");
    }

    #[test]
    fn a_refused_request_surfaces_its_status_and_no_body() {
        let (base, _seen) = spawn_mock(
            "HTTP/1.1 429 Too Many Requests",
            "Content-Length: 24\r\nConnection: close\r\n",
            "{\"error\":\"slow down\"}",
        );
        let prepared = prepared_for(&base, Provider::Anthropic, None);
        let events = run_against(&prepared, Arc::new(AtomicBool::new(false)));
        assert_eq!(events.len(), 1);
        assert!(events[0].contains("429"), "got: {events:?}");
        assert!(!events[0].contains("slow down"), "got: {events:?}");
    }

    /// Every `tracing` call this module makes, run under a capturing
    /// subscriber. The lines are written on this thread rather than through
    /// the request task, because the subscriber a test installs is
    /// thread-local and a line written on a runtime worker would escape it.
    #[test]
    fn no_key_prompt_note_or_reply_reaches_a_log_line() {
        let prepared = prepared_for("http://127.0.0.1:1", Provider::Anthropic, Some(SECRET_KEY));
        let logs = captured_logs(|| {
            log_request(&prepared, NOTE_TEXT.len(), 1);
            log_rejected(401);
            record_proposal(
                Path::new("/nowhere/at/all"),
                "127.0.0.1",
                "apply_proposal",
                "Ideas/Launch.md",
                Decision::Allow,
                Some(12),
            );
        });
        assert!(!logs.contains(SECRET_KEY), "a key reached the log: {logs}");
        assert!(!logs.contains(NOTE_TEXT), "a note reached the log: {logs}");
        assert!(
            !logs.contains(USER_TURN),
            "a prompt reached the log: {logs}"
        );
        assert!(
            !logs.contains(RECORDED_REPLY),
            "a reply reached the log: {logs}"
        );
        assert!(
            !logs.contains(writ_core::chat::SYSTEM_PROMPT),
            "the system prompt reached the log: {logs}"
        );
        assert!(logs.contains("127.0.0.1"), "the host is loggable: {logs}");
        assert!(logs.contains("401"), "the status is loggable: {logs}");
    }
}

/// Fixtures the streaming tests share.
#[cfg(test)]
mod tests_support {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// The same recorded frames `writ-core` reads its grammar against, served
    /// over a socket so the streaming path is exercised without a network.
    pub const ANTHROPIC_STREAM: &str =
        include_str!("../../../crates/writ-core/tests/fixtures/chat/anthropic-stream.sse");

    /// The reply those frames spell out.
    pub const RECORDED_REPLY: &str = "The note argues one thing.";

    pub const SECRET_KEY: &str = "sk-do-not-log-me";
    pub const NOTE_TEXT: &str = "the note said this and it is nobody else's business";
    pub const USER_TURN: &str = "what does the note argue about the launch";

    /// A one-shot server that answers with `body` and keeps what it was sent.
    pub fn spawn_mock(
        status_line: &'static str,
        headers: &'static str,
        body: &'static str,
    ) -> (String, Arc<Mutex<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let seen = Arc::new(Mutex::new(String::new()));
        let recorder = seen.clone();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 8192];
                let read = stream.read(&mut buf).unwrap_or_default();
                *recorder.lock().expect("request") =
                    String::from_utf8_lossy(&buf[..read]).into_owned();
                let response = format!("{status_line}\r\n{headers}\r\n{body}");
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (format!("http://127.0.0.1:{port}"), seen)
    }

    /// A request aimed at a local mock, carrying a note, a turn and a key.
    pub fn prepared_for(base: &str, provider: Provider, key: Option<&str>) -> PreparedChat {
        let context = vec![AttachedNote {
            path: "Ideas/Launch.md".to_string(),
            text: NOTE_TEXT.to_string(),
            before_hash: writ_core::hash::sha256_hex(NOTE_TEXT.as_bytes()),
        }];
        let turns = vec![ChatTurn {
            role: writ_core::chat::Role::User,
            content: USER_TURN.to_string(),
        }];
        PreparedChat {
            endpoint: chat::endpoint(provider, base),
            provider,
            body: chat::build_request_body(
                provider,
                "a-model",
                chat::SYSTEM_PROMPT,
                &turns,
                &context,
            ),
            api_key: key.map(str::to_string),
            host: "127.0.0.1".to_string(),
            is_localhost: true,
            context,
        }
    }

    /// What `tracing` wrote while `run` ran.
    pub fn captured_logs(run: impl FnOnce()) -> String {
        let buffer = Arc::new(Mutex::new(Vec::<u8>::new()));
        let sink = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || Sink(sink.clone()))
            .finish();
        tracing::subscriber::with_default(subscriber, run);
        let bytes = buffer.lock().expect("logs").clone();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// The capturing subscriber's writer.
    pub struct Sink(pub Arc<Mutex<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("logs").extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}
