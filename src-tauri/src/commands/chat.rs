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
//! [`super::ai`]'s and are reused here (ADR-031 rules 2.6 and 6.1), and every
//! note this module reads or writes is reached through the note host, holding
//! the narrowest set of capabilities the surface needs (ADR-032).
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
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use writ_core::activity::{ActivityRecord, Actor, Decision};
use writ_core::ai::models::{CatalogSource, ModelCatalog};
use writ_core::chat::{
    self, AssistantReply, AttachedNote, AttachmentRef, ChatError, ChatErrorFrame, ChatTurn,
    Conversation, Delta, ParsedProposals, ProposalFilter, ProposalStatus, Provider, RejectCode,
    RequestIdentity, Role, StoredProposal, MAX_CONVERSATION_BYTES,
};
use writ_core::config::AiConfig;
use writ_core::diff::{line_diff, Hunk};
use writ_core::hash::digest_from_hex;
use writ_core::notes::host::{Capability, HostError, NoteHost, PermissionSet};
use writ_core::notes::WriteOrigin;
use writ_core::polish;
use writ_storage::chat_store::{ChatStore, ChatStoreError, ConversationSummary};
use writ_storage::note_host::NoteHostImpl;
use writ_storage::paths::{file_name_only, relative_slug};

use super::ai::AiKeyState;
use crate::events::{emit_event, WritFrontendEvent};
use crate::poison::recover_poison;
use crate::state::AppState;
use crate::watcher::open_files::OpenNotes;

/// How many notes one request may carry, so a loop in a caller cannot assemble
/// an unbounded body.
const MAX_ATTACHED_NOTES: usize = 20;

/// What a send says when the id it names has no file.
///
/// The pane keeps its conversation in a file the store owns, so an id nothing
/// answers to is a conversation that was deleted, not a request that failed.
const MISSING_CONVERSATION: &str = "This chat no longer exists.";

/// What a send says when one more turn would not fit in the file
/// (ADR-040 section 8).
const CONVERSATION_FULL: &str = "This chat is full. Start a new chat to continue.";

/// What a second send says while the reply to the first is still streaming.
pub const REPLY_IN_FLIGHT: &str = "A reply is still arriving.";

/// The request a conversation is running: the id the pane minted for it and
/// the flag that stops it.
///
/// The id is what makes a stop and a finish nameable. Without it a stop sent
/// while one reply was ending and the next beginning reaches whichever request
/// happened to be in the map.
struct LiveRequest {
    request_id: String,
    cancel: Arc<AtomicBool>,
}

/// Session-scoped state for the pane: the live request of each conversation,
/// keyed by the conversation the frontend named.
///
/// Cloning shares one registry, so a task can hold its own handle rather than
/// borrowing the managed state for as long as it streams.
#[derive(Default, Clone)]
pub struct ChatState {
    tasks: Arc<Mutex<HashMap<String, LiveRequest>>>,
}

impl ChatState {
    /// Claims a conversation for one request and hands back its cancel flag,
    /// or `None` when a reply is already arriving for it.
    ///
    /// The check and the claim are one lock. Two sends racing on one
    /// conversation are two tasks appending to one file, and the loser must
    /// learn it lost before it reads a note or a key, not after.
    pub fn try_begin(&self, conversation_id: &str, request_id: &str) -> Option<Arc<AtomicBool>> {
        let mut tasks = recover_poison(self.tasks.lock(), "commands::chat::try_begin");
        match tasks.entry(conversation_id.to_string()) {
            std::collections::hash_map::Entry::Occupied(_) => None,
            std::collections::hash_map::Entry::Vacant(slot) => {
                let cancel = Arc::new(AtomicBool::new(false));
                slot.insert(LiveRequest {
                    request_id: request_id.to_string(),
                    cancel: cancel.clone(),
                });
                Some(cancel)
            }
        }
    }

    /// Raises the cancel flag of the named request, or of whatever is live
    /// when no request is named.
    ///
    /// `false` when nothing matches: a stop that arrives after its reply
    /// finished, or one naming a request the conversation has already moved
    /// on from. Shutdown is the caller with no id, because it stops every
    /// reply rather than one it chose.
    pub fn cancel(&self, conversation_id: &str, request_id: Option<&str>) -> bool {
        let tasks = recover_poison(self.tasks.lock(), "commands::chat::cancel");
        match tasks.get(conversation_id) {
            Some(live) if request_id.is_none_or(|named| named == live.request_id) => {
                live.cancel.store(true, Ordering::Relaxed);
                true
            }
            _ => false,
        }
    }

    /// Forgets a conversation's request, and only that request.
    ///
    /// A finish that arrives after the conversation moved on removes nothing:
    /// the entry it would take belongs to a send that is still streaming.
    fn release(&self, conversation_id: &str, request_id: &str) {
        let mut tasks = recover_poison(self.tasks.lock(), "commands::chat::release");
        if tasks
            .get(conversation_id)
            .is_some_and(|live| live.request_id == request_id)
        {
            tasks.remove(conversation_id);
        }
    }

    /// Whether a reply is arriving for that conversation.
    ///
    /// True from the moment a send registers it until its task has ended,
    /// a stop included: what a stop asks for is the end of the stream, and the
    /// reply is still being written to the file until it comes.
    pub fn is_live(&self, conversation_id: &str) -> bool {
        recover_poison(self.tasks.lock(), "commands::chat::is_live").contains_key(conversation_id)
    }

    /// How many conversations are live.
    pub fn live(&self) -> usize {
        recover_poison(self.tasks.lock(), "commands::chat::live").len()
    }

    /// Stops every live reply, and says how many it stopped.
    ///
    /// Shutdown's form of the stop: it names no request because it means all
    /// of them, and each task then saves what arrived through the same
    /// stopped path a person's Stop uses.
    pub fn cancel_all(&self) -> usize {
        let tasks = recover_poison(self.tasks.lock(), "commands::chat::cancel_all");
        for live in tasks.values() {
            live.cancel.store(true, Ordering::Relaxed);
        }
        tasks.len()
    }
}

/// How long a quit waits for the replies it stopped to write what arrived.
///
/// The text a reply has streamed so far is held by the task streaming it, and
/// reaches the conversation file only when that task ends. The budget is the
/// whole wait rather than one per conversation: a quit that hangs on a stalled
/// host is worse than a reply that lost its last few words.
pub const CHAT_SHUTDOWN_BUDGET: Duration = Duration::from_millis(500);

/// How often the wait looks again. Short enough that the usual case — a task
/// that ends in a millisecond or two — costs the quit nothing measurable.
const CHAT_SHUTDOWN_POLL: Duration = Duration::from_millis(5);

/// Stops every live reply and waits, briefly, for their tasks to save.
///
/// `true` when every conversation released inside the budget. `false` is a
/// reply whose host is still holding the connection open: its partial text is
/// lost, which is what the pre-stop behaviour did to every interrupted reply.
pub fn stop_live_chats(chat: &ChatState, budget: Duration) -> bool {
    if chat.cancel_all() == 0 {
        return true;
    }
    let deadline = Instant::now() + budget;
    loop {
        let left = chat.live();
        if left == 0 {
            return true;
        }
        if Instant::now() >= deadline {
            tracing::warn!(
                conversations = left,
                "a reply was still arriving when the quit ran out of time for it"
            );
            return false;
        }
        std::thread::sleep(CHAT_SHUTDOWN_POLL);
    }
}

/// Holds a conversation for one request and releases it however the request
/// ends.
///
/// A plain call at the end of the task released the conversation only on the
/// paths that reached it: a panic in the stream, in the parse or in the save
/// left the conversation live for the life of the process, with every later
/// send refused and no stop able to clear it. A refusal between the claim and
/// the spawn had the same shape. Dropping is the one release, so there is no
/// path that forgets it.
pub struct LiveGuard {
    state: ChatState,
    conversation_id: String,
    request_id: String,
}

impl LiveGuard {
    pub fn new(state: ChatState, conversation_id: String, request_id: String) -> Self {
        Self {
            state,
            conversation_id,
            request_id,
        }
    }
}

impl Drop for LiveGuard {
    fn drop(&mut self) {
        self.state.release(&self.conversation_id, &self.request_id);
    }
}

/// Claims a conversation for one send, or refuses it.
///
/// The claim and the guard come together: every path out of a send, accepted
/// or refused, carries the guard, so the entry is the task's for exactly as
/// long as the task exists.
pub fn begin_request(
    chat: &ChatState,
    conversation_id: &str,
    request_id: &str,
) -> Result<(Arc<AtomicBool>, LiveGuard), String> {
    match chat.try_begin(conversation_id, request_id) {
        Some(cancel) => Ok((
            cancel,
            LiveGuard::new(
                chat.clone(),
                conversation_id.to_string(),
                request_id.to_string(),
            ),
        )),
        None => Err(REPLY_IN_FLIGHT.to_string()),
    }
}

/// Where the chat endpoint points and what it still needs.
///
/// Mirrors [`super::ai::AiEndpointState`] rather than sharing it: the pane
/// reads its own switch and may name a model of its own, so the two answers
/// differ even though the connection behind them is one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatEndpointState {
    /// Whether `[ai.chat] enabled` is on.
    pub enabled: bool,
    /// The connection's provider id.
    pub provider: String,
    /// The model chat sends: its own when it names one, the connection's
    /// otherwise.
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

/// One conversation as the pane reads it.
///
/// The stored document with each pending proposal compared against the note as
/// it stands now: the file holds no note text, so a hunk list is a view built
/// when the conversation is opened rather than something it remembers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConversationDto {
    /// The conversation's id, which is also the stream key.
    pub id: String,
    /// What the pane calls it.
    pub title: String,
    /// When it was created, RFC 3339.
    pub created_at: String,
    /// When it last changed, RFC 3339.
    pub updated_at: String,
    /// The provider its turns were sent to.
    pub provider: String,
    /// The model its turns were sent to.
    pub model: String,
    /// The turns, oldest first.
    pub turns: Vec<TurnDto>,
}

/// One turn as the pane reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TurnDto {
    /// Who said it.
    pub role: Role,
    /// What was said, with any proposal already removed.
    pub content: String,
    /// The notes a user turn put in front of the model, by name and digest.
    pub attachments: Vec<AttachmentRef>,
    /// The changes an assistant turn asked for.
    pub proposals: Vec<ProposalDto>,
}

/// One proposal as the card shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProposalDto {
    /// The note it changes, folder-relative.
    pub path: String,
    /// The one line the model gave for it, empty when it gave none.
    pub summary: String,
    /// What the note held when the request was built.
    pub before_hash: String,
    /// The whole text the note would hold.
    pub new_content: String,
    /// What became of it.
    pub status: ProposalStatus,
    /// The change against the note as it stands now, empty for a proposal
    /// nobody can apply any more and for a note too large to compare.
    pub hunks: Vec<Hunk>,
    /// The note has moved on since the proposal was made, so applying it would
    /// be refused by the guard.
    pub stale: bool,
}

/// What a send was accepted as: the id its frames carry, and the notes the
/// request was built from.
///
/// The notes come back so the pane can show a proposal beside what the model
/// actually read, which is the text `before_hash` describes rather than
/// whatever the file holds by the time the reply lands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatSendAccepted {
    /// The conversation the frames are keyed by.
    pub conversation_id: String,
    /// The send the frames belong to, echoed back so the pane can pin the
    /// exchange it is showing to the request that will fill it.
    pub request_id: String,
    /// What the request carried, in the order it carried it.
    pub attached: Vec<AttachedNote>,
    /// The connection the request was frozen against: what the reply will be
    /// attributed to, whatever the config says by the time it lands.
    pub identity: RequestIdentity,
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
    /// The write moved bytes.
    ///
    /// False when the note already held the proposed text, which is what a
    /// model asked for a whole note often writes back. The card says so rather
    /// than reporting a change nobody made.
    pub changed: bool,
}

/// Builds the endpoint state for `cfg`. Pure over its key lookup, so the
/// consent/key matrix is testable without a keychain.
pub fn endpoint_state_from(cfg: &AiConfig, key_state: AiKeyState) -> ChatEndpointState {
    let base = ChatEndpointState {
        enabled: cfg.chat.enabled,
        provider: cfg.provider.clone(),
        model: cfg.chat_model().to_string(),
        host: None,
        host_port: None,
        is_hosted: false,
        is_allowed: false,
        is_consented: false,
        key_state,
    };
    match polish::resolve_endpoint(&cfg.effective_base_url()) {
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
    match polish::resolve_endpoint(&cfg.effective_base_url()) {
        Ok(target) => target.is_hosted,
        Err(_) => false,
    }
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
    let key_state = if needs_key_lookup(&cfg) {
        super::ai::key_state_for(&app, &cfg.provider)
    } else {
        AiKeyState {
            is_set: false,
            memory_only: false,
        }
    };
    endpoint_state_from(&cfg, key_state)
}

/// The sizes the send dialog must state, read off disk at the moment it asks.
#[tauri::command]
pub fn chat_attached_sizes(
    app: AppHandle,
    paths: Vec<String>,
) -> Result<Vec<AttachedSize>, String> {
    let state = app.state::<AppState>();
    attached_sizes_in(&state.notes_root(), &state.open_tabs(), &paths)
}

/// The conversation folder under this instance's data directory.
fn chat_store(app: &AppHandle) -> ChatStore {
    ChatStore::new(&app.state::<AppState>().writ_dir)
}

/// The conversations the folder holds, most recently changed first.
#[tauri::command]
pub fn chat_list(app: AppHandle) -> Result<Vec<ConversationSummary>, String> {
    chat_store(&app).list().map_err(|error| error.to_string())
}

/// One conversation, with every pending proposal read against the note as it
/// stands now.
#[tauri::command]
pub fn chat_open(app: AppHandle, id: String) -> Result<ConversationDto, String> {
    let conversation = chat_store(&app).load(&id).map_err(missing_or)?;
    let state = app.state::<AppState>();
    Ok(conversation_dto(
        &state.notes_root(),
        &state.open_tabs(),
        conversation,
    ))
}

/// An empty conversation, on disk before the pane sees it.
#[tauri::command]
pub fn chat_new(app: AppHandle) -> Result<ConversationDto, String> {
    let cfg = chat_config(&app);
    let made = chat_store(&app)
        .create(&cfg.provider, cfg.chat_model())
        .map_err(|error| error.to_string())?;
    let state = app.state::<AppState>();
    Ok(conversation_dto(
        &state.notes_root(),
        &state.open_tabs(),
        made,
    ))
}

/// Names a conversation. A title that holds nothing leaves the name it has.
#[tauri::command]
pub fn chat_rename(app: AppHandle, id: String, title: String) -> Result<ConversationDto, String> {
    let renamed = chat_store(&app).rename(&id, &title).map_err(missing_or)?;
    let state = app.state::<AppState>();
    Ok(conversation_dto(
        &state.notes_root(),
        &state.open_tabs(),
        renamed,
    ))
}

/// Unlinks a conversation. There is no trash for one, because it is not a note.
#[tauri::command]
pub fn chat_delete(app: AppHandle, id: String) -> Result<(), String> {
    chat_delete_inner(&chat_store(&app), &app.state::<ChatState>(), &id)
}

/// Stops the conversation's reply, then unlinks it.
///
/// The stop comes first because a reply outliving its file has nowhere to go:
/// its chunks reach a pane with no conversation to put them in, and the save
/// that ends it fails on a file nothing can recreate.
pub fn chat_delete_inner(store: &ChatStore, chat: &ChatState, id: &str) -> Result<(), String> {
    chat.cancel(id, None);
    store.delete(id).map_err(missing_or)
}

/// Renders one reply to the fragment the pane inserts into its own DOM.
///
/// The untrusted variant: a model's reply is untrusted input (ADR-031 rule
/// 4.1), so raw HTML is dropped rather than passed through, and the pane loads
/// neither the diagram runtime nor the math one.
#[tauri::command]
pub fn chat_render_reply(markdown: String) -> String {
    writ_render::render_markdown_fragment_untrusted(&markdown).html
}

/// A store failure the pane can act on: an id nothing answers to reads as a
/// conversation that is gone, and everything else as itself.
fn missing_or(error: ChatStoreError) -> String {
    match error {
        ChatStoreError::NotFound(_) | ChatStoreError::InvalidId(_) => {
            MISSING_CONVERSATION.to_string()
        }
        other => other.to_string(),
    }
}

/// The stored conversation as the pane reads it.
///
/// A pending proposal is compared against the note **as it stands now**, not
/// against what the model was shown: the send-time text is deliberately not
/// persisted (ADR-040 section 8), so the note on disk is the only baseline
/// there is. `stale` is what says the note moved, and it is also what the
/// guard will refuse the apply for. A proposal nobody can apply any more — one
/// already applied, discarded or refused — carries no hunks, and the card
/// shows its summary and its status.
fn conversation_dto(
    notes_root: &Path,
    open_notes: &dyn OpenNotes,
    conversation: Conversation,
) -> ConversationDto {
    ConversationDto {
        id: conversation.id,
        title: conversation.title,
        created_at: conversation.created_at,
        updated_at: conversation.updated_at,
        provider: conversation.provider,
        model: conversation.model,
        turns: conversation
            .turns
            .into_iter()
            .map(|turn| TurnDto {
                role: turn.role,
                content: turn.content,
                attachments: turn.attachments,
                proposals: turn
                    .proposals
                    .into_iter()
                    .map(|proposal| proposal_dto(notes_root, open_notes, proposal))
                    .collect(),
            })
            .collect(),
    }
}

/// One stored proposal, with the diff it would make computed now.
fn proposal_dto(
    notes_root: &Path,
    open_notes: &dyn OpenNotes,
    proposal: StoredProposal,
) -> ProposalDto {
    if proposal.status != ProposalStatus::Pending {
        return ProposalDto {
            path: proposal.path,
            summary: proposal.summary,
            before_hash: proposal.before_hash,
            new_content: proposal.new_content,
            status: proposal.status,
            hunks: Vec::new(),
            stale: false,
        };
    }

    // A note that cannot be read is a note the proposal can no longer be
    // measured against, which is the same thing to a reader as one that
    // changed: there is nothing to show but the summary, and applying will be
    // refused.
    let Some(current) = note_now(notes_root, open_notes, &proposal.path) else {
        return ProposalDto {
            path: proposal.path,
            summary: proposal.summary,
            before_hash: proposal.before_hash,
            new_content: proposal.new_content,
            status: proposal.status,
            hunks: Vec::new(),
            stale: true,
        };
    };

    let hunks = line_diff(&current.text, &proposal.new_content).unwrap_or_default();
    ProposalDto {
        stale: current.hash != proposal.before_hash,
        path: proposal.path,
        summary: proposal.summary,
        before_hash: proposal.before_hash,
        new_content: proposal.new_content,
        status: proposal.status,
        hunks,
    }
}

/// What one note holds now, read through the context host and nothing else.
///
/// Every refusal collapses to `None`: opening a conversation must not fail
/// because a note it names was renamed, deleted or grew past what a host will
/// read.
fn note_now(
    notes_root: &Path,
    open_notes: &dyn OpenNotes,
    path: &str,
) -> Option<writ_core::notes::host::NoteContent> {
    let context = resolve_context_file(notes_root, open_notes, path).ok()?;
    read_context(notes_root, &context, path).ok()
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
    /// The host with its port, for the line a local runtime's silence reads
    /// as.
    pub host_port: String,
    /// The endpoint is on this machine.
    pub is_localhost: bool,
    /// The notes the request carries, and nothing else.
    pub context: Vec<AttachedNote>,
    /// The connection this request was frozen against, carried on every frame
    /// it produces.
    pub identity: RequestIdentity,
}

/// Validates config and inputs and resolves the request.
///
/// `lookup_key` maps a provider id to its key and is consulted only for a
/// hosted endpoint, after consent, so a refused request reads no credential.
pub fn prepare_chat(
    cfg: &AiConfig,
    turns: &[ChatTurn],
    context: Vec<AttachedNote>,
    catalog: Option<&ModelCatalog>,
    lookup_key: impl FnOnce(&str) -> Option<String>,
) -> Result<PreparedChat, ChatError> {
    if !cfg.chat.enabled {
        return Err(ChatError::Disabled);
    }
    let provider = Provider::from_wire(cfg.wire());
    if turns
        .iter()
        .last()
        .is_none_or(|turn| turn.content.trim().is_empty())
    {
        return Err(ChatError::EmptyMessage);
    }

    // The one authority: the guard here and `ai_consent_host` resolve the host
    // through the same call, so the string checked is the string recorded.
    let base_url = cfg.effective_base_url();
    let target = polish::resolve_endpoint(&base_url)?;
    if !target.is_allowed {
        return Err(ChatError::EndpointNotAllowed);
    }
    if cfg.chat_model().trim().is_empty() {
        return Err(ChatError::ModelRequired);
    }
    model_is_on_offer(cfg, catalog)?;

    let api_key = if target.is_hosted {
        if !super::ai::is_consented(cfg, &target.host) {
            return Err(ChatError::ConsentRequired {
                host: target.host.clone(),
            });
        }
        match lookup_key(&cfg.provider) {
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
        cfg.chat_model(),
        &chat::system_prompt(&context),
        turns,
        &context,
    );
    Ok(PreparedChat {
        endpoint: chat::endpoint(provider, &base_url),
        provider,
        body,
        api_key,
        is_localhost: !target.is_hosted,
        identity: RequestIdentity {
            provider: cfg.provider.clone(),
            model: cfg.chat_model().to_string(),
            host: target.host.clone(),
        },
        host: target.host,
        host_port: target.host_port,
        context,
    })
}

/// Whether the model about to be sent is one the provider offers.
///
/// Only a live catalog read for this same provider can answer: the table's
/// suggestions are guesses, and a list answered for another provider describes
/// another server. A catalog that cannot answer blocks nothing, so a send is
/// never refused for want of evidence.
fn model_is_on_offer(cfg: &AiConfig, catalog: Option<&ModelCatalog>) -> Result<(), ChatError> {
    let Some(catalog) = catalog else {
        return Ok(());
    };
    if catalog.provider != cfg.provider || catalog.source != CatalogSource::Live {
        return Ok(());
    }
    if catalog.models.is_empty() {
        return Err(ChatError::EmptyModelList {
            provider: cfg.provider.clone(),
        });
    }
    if catalog.refuses(cfg.chat_model()) {
        return Err(ChatError::ModelUnavailable {
            model: cfg.chat_model().to_string(),
            provider: cfg.provider.clone(),
        });
    }
    Ok(())
}

/// The notes folder in the one spelling every path here is compared against.
///
/// `resolve_for_containment` hands back a canonical path with the Windows
/// `\\?\` prefix dropped, and the root the app carries has been through
/// neither step: on Windows `\\?\C:\notes` never prefixes `C:\notes\a.md`, so
/// a note plainly in the folder is refused. Both sides go through the same
/// canonicalisation before they meet. A root the filesystem cannot resolve is
/// compared as it stands, which is what the check did before.
fn canonical_notes_root(notes_root: &Path) -> PathBuf {
    crate::security::canonicalize_root(notes_root).unwrap_or_else(|_| notes_root.to_path_buf())
}

/// The note file a folder-relative (or absolute) path names.
///
/// Every existing part of the path is canonicalised before the containment
/// check, so neither the file nor a linked directory above it can carry an
/// answer out of the notes folder.
pub fn note_file_in(notes_root: &Path, path: &str) -> Result<PathBuf, String> {
    let root = canonical_notes_root(notes_root);
    let given = Path::new(path);
    let candidate = match given.is_absolute() {
        true => given.to_path_buf(),
        false => root.join(given),
    };
    let Some(resolved) = crate::security::resolve_for_containment(&candidate) else {
        return Err(outside_notes(path));
    };
    if !writ_core::notes::containment::is_inside(&root, Path::new(&resolved)) {
        return Err(outside_notes(path));
    }
    let file = PathBuf::from(resolved);
    // The sentence goes under a chip or a card that already names the note,
    // so it says what is wrong and not, a second time, which note.
    if !file.exists() {
        return Err("This note is no longer there.".to_string());
    }
    if !file.is_file() {
        return Err("This is not a note.".to_string());
    }
    Ok(file)
}

/// A refusal names the note, never the folder it was looked for in.
///
/// The path that reaches here is the caller's own, which for the pane is the
/// absolute source path of an open tab. Where the folder sits on this machine
/// is not something a person needs from the sentence, and it is not something
/// a model reading the pane should be handed either (ADR-031 rule 5.2).
fn outside_notes(path: &str) -> String {
    format!("{} is not in the notes folder.", file_name_only(path))
}

/// The note's path as the pane lists it and a proposal names it: relative to
/// the notes folder, so nothing that leaves the machine carries the absolute
/// path of the folder it came from.
///
/// [`note_file_in`] has already established that the folder holds the file, so
/// the prefix is there to strip. It is an error rather than a fallback because
/// the only thing left to fall back to is the bare file name, and that would
/// give `Ideas/Launch.md` and `Archive/Launch.md` one key: two different notes
/// the pane could not tell apart and a proposal could apply to the wrong one.
fn relative_key(notes_root: &Path, file: &Path) -> Result<String, String> {
    let root = canonical_notes_root(notes_root);
    relative_slug(&root, file).ok_or_else(|| outside_notes(&file.to_string_lossy()))
}

/// Whether a context file is a note in the folder or a file that is reachable
/// only because a tab has it open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextScope {
    /// A note the folder holds. Read and written through the note host.
    Notes,
    /// A file outside the folder, open in this tab. Read directly and written
    /// through the tab's own save path.
    OpenTab { tab_id: String },
}

/// One file the chat may read, with the name every chat surface calls it by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextFile {
    /// The file on disk, resolved.
    pub file: PathBuf,
    /// Folder-relative for a note, the absolute path for a file outside the
    /// folder. This is the string the chip, the conversation file, the
    /// proposal and the activity record all name the file by.
    pub key: String,
    /// Which of the two ways this file is reached.
    pub scope: ContextScope,
}

/// The one place a chat path becomes a file the pane may read.
///
/// A note in the folder resolves exactly as [`note_file_in`] and
/// [`relative_key`] resolved it together. A path outside the folder is
/// accepted only when a tab has it open: the user opened the file and can
/// already save it, so the pane adds no reach the editor lacks (ADR-040
/// section 13). A path outside the folder that nobody has open is refused in
/// the sentence it has always been refused in, which is why the tab lookup
/// comes before the existence checks — a traversal at a path holding nothing
/// must not start answering "This note is no longer there.".
pub fn resolve_context_file(
    notes_root: &Path,
    open_notes: &dyn OpenNotes,
    path: &str,
) -> Result<ContextFile, String> {
    let root = canonical_notes_root(notes_root);
    let given = Path::new(path);
    let candidate = match given.is_absolute() {
        true => given.to_path_buf(),
        false => root.join(given),
    };
    let Some(resolved) = crate::security::resolve_for_containment(&candidate) else {
        return Err(outside_notes(path));
    };
    let file = PathBuf::from(resolved);

    if writ_core::notes::containment::is_inside(&root, &file) {
        let key = existing_file(&file).and_then(|()| relative_key(&root, &file))?;
        return Ok(ContextFile {
            file,
            key,
            scope: ContextScope::Notes,
        });
    }

    let Some(tab_id) = open_notes.note_at(&file) else {
        return Err(outside_notes(path));
    };
    existing_file(&file)?;
    Ok(ContextFile {
        key: file.to_string_lossy().into_owned(),
        file,
        scope: ContextScope::OpenTab { tab_id },
    })
}

/// The two refusals a path that resolved but holds no file earns.
///
/// The sentences go under a chip or a card that already names the file, so
/// they say what is wrong and not, a second time, which file.
fn existing_file(file: &Path) -> Result<(), String> {
    if !file.exists() {
        return Err("This note is no longer there.".to_string());
    }
    if !file.is_file() {
        return Err("This is not a note.".to_string());
    }
    Ok(())
}

/// What a refusal about a context file calls it.
///
/// A note reads by its folder-relative key, as every refusal has always read.
/// A file outside the folder reads by its file name: the absolute path belongs
/// in the chip's tooltip, the conversation file and the activity record, and
/// not in a sentence a model's reply sits next to (ADR-031 rule 5.2).
fn context_name(context: &ContextFile) -> String {
    match context.scope {
        ContextScope::Notes => context.key.clone(),
        ContextScope::OpenTab { .. } => file_name_only(&context.key),
    }
}

/// A note's size on disk, as the dialog that asks to send it must state it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachedSize {
    /// The path the caller asked about, echoed back exactly as it was given.
    ///
    /// The pane holds absolute source paths and this command answers about
    /// notes it resolves and keys by folder-relative name; a caller joining
    /// its own list to this answer needs a string it already has, or the join
    /// misses on every row and quietly keeps whatever it started with.
    pub path: String,
    /// The note's folder-relative key, which is how every other chat surface
    /// names it.
    pub key: String,
    /// What the file holds now, which is what a send would carry.
    pub bytes: u64,
}

/// What the side a model's reply can influence may ask for.
///
/// One capability. The pane attaches the tabs the user named and lists nothing,
/// and a capability with no caller is not held. There is no write here, which is
/// what turns ADR-031 rule 4.3 into a property of the type rather than a habit
/// of the code.
pub fn context_permissions() -> PermissionSet {
    [Capability::ReadNote].into_iter().collect()
}

/// What applying a proposal may ask for.
///
/// Applying runs from the user's `Apply` and from nothing the model produced,
/// so it is the one place the pane holds a write and it holds nothing else.
pub fn apply_permissions() -> PermissionSet {
    [Capability::WriteNote].into_iter().collect()
}

/// A host over the notes folder holding [`context_permissions`].
///
/// Opened per note rather than per call: a call that names no note reads
/// nothing, and the folder has already been resolved by the time this is asked
/// for.
fn context_host(notes_root: &Path, note_key: &str) -> Result<NoteHostImpl<'static>, String> {
    NoteHostImpl::open(notes_root, None, context_permissions())
        .map_err(|_| format!("{note_key} could not be read."))
}

/// What the pane shows when a note it was told to attach does not come back.
///
/// The pane names notes by their folder-relative key everywhere else, and a
/// host answer names the path the caller handed in, which for the pane is the
/// absolute source path of an open tab (ADR-031 rule 5.2).
fn unattachable(note_key: &str, error: &HostError) -> String {
    match error {
        HostError::TooLarge { .. } => format!("{note_key} is too large to attach."),
        HostError::NotText { .. } => format!("{note_key} is not text."),
        _ => format!("{note_key} could not be read."),
    }
}

/// What one context file holds now.
///
/// A note goes through the host, which is the only thing that reads the notes
/// folder. A file outside the folder is read directly, under the host's own
/// ceiling ([`writ_core::notes::host::MAX_NOTE_BYTES`]), the host's own
/// text-or-not answer, and the host's own digest, so a `before_hash` taken
/// here is comparable with one taken through the host and the two sides refuse
/// an oversized or binary file in the same words.
fn read_context(
    notes_root: &Path,
    context: &ContextFile,
    given: &str,
) -> Result<writ_core::notes::host::NoteContent, String> {
    match &context.scope {
        ContextScope::Notes => context_host(notes_root, &context.key)?
            .read_note(given)
            .map_err(|error| unattachable(&context.key, &error)),
        ContextScope::OpenTab { .. } => {
            let name = context_name(context);
            let bytes = std::fs::metadata(&context.file)
                .map_err(|_| format!("{name} could not be read."))?
                .len();
            if bytes > writ_core::notes::host::MAX_NOTE_BYTES {
                return Err(format!("{name} is too large to attach."));
            }
            let read =
                std::fs::read(&context.file).map_err(|_| format!("{name} could not be read."))?;
            let text = String::from_utf8(read).map_err(|_| format!("{name} is not text."))?;
            Ok(writ_core::notes::host::NoteContent {
                path: context.key.clone(),
                bytes,
                hash: writ_core::hash::sha256_hex(text.as_bytes()),
                text,
            })
        }
    }
}

/// What one context file weighs, which is what the send dialog states.
///
/// Metadata on both sides, so asking costs no file text. A file over the
/// ceiling still reports its size here and is refused at the read, which is
/// what a note has always done: `note_summary` carries no size limit either.
fn context_bytes(notes_root: &Path, context: &ContextFile, given: &str) -> Result<u64, String> {
    match &context.scope {
        ContextScope::Notes => context_host(notes_root, &context.key)?
            .note_summary(given)
            .map(|summary| summary.bytes)
            .map_err(|error| unattachable(&context.key, &error)),
        ContextScope::OpenTab { .. } => std::fs::metadata(&context.file)
            .map(|meta| meta.len())
            .map_err(|_| format!("{} could not be read.", context_name(context))),
    }
}

/// The sizes of the notes the call named.
///
/// The dialog asking to send them must state the bytes the send will read, not
/// what a tab last recorded: a note another program rewrote since the tab
/// synced would otherwise be consented to under the wrong number. Metadata
/// only, so asking costs no note text.
pub fn attached_sizes_in(
    notes_root: &Path,
    open_notes: &dyn OpenNotes,
    paths: &[String],
) -> Result<Vec<AttachedSize>, String> {
    if paths.len() > MAX_ATTACHED_NOTES {
        return Err(format!(
            "Attach at most {MAX_ATTACHED_NOTES} notes to one conversation."
        ));
    }
    let mut sizes: Vec<AttachedSize> = Vec::with_capacity(paths.len());
    for path in paths {
        let context = resolve_context_file(notes_root, open_notes, path)?;
        // Two spellings of one file are one row, and the first spelling asked
        // about is the one answered under.
        if sizes.iter().any(|note| note.key == context.key) {
            continue;
        }
        let bytes = context_bytes(notes_root, &context, path)?;
        sizes.push(AttachedSize {
            path: path.clone(),
            key: context.key,
            bytes,
        });
    }
    Ok(sizes)
}

/// Reads the notes the call named, in the order it named them.
///
/// This is the whole of what the request carries. Nothing walks the folder and
/// nothing consults the index (ADR-031 rule 2.5).
pub fn read_attached_in(
    notes_root: &Path,
    open_notes: &dyn OpenNotes,
    paths: &[String],
) -> Result<Vec<AttachedNote>, String> {
    if paths.len() > MAX_ATTACHED_NOTES {
        return Err(format!(
            "Attach at most {MAX_ATTACHED_NOTES} notes to one conversation."
        ));
    }
    let mut notes: Vec<AttachedNote> = Vec::with_capacity(paths.len());
    for path in paths {
        let context = resolve_context_file(notes_root, open_notes, path)?;
        if notes.iter().any(|note| note.path == context.key) {
            continue;
        }
        let content = read_context(notes_root, &context, path)?;
        // The key is what an apply writes to and what the conversation file
        // records; what the model is told is the name it can say back. For a
        // note those are one string. For a file outside the folder the key is
        // the whole path, and where that file sits on this machine is not the
        // model's to read (ADR-031 rule 2.5).
        let prompt_path = match context.scope {
            ContextScope::Notes => context.key.clone(),
            ContextScope::OpenTab { .. } => outside_prompt_path(&context.key),
        };
        notes.push(AttachedNote {
            before_hash: content.hash,
            path: context.key,
            prompt_path,
            text: content.text,
        });
    }
    Ok(notes)
}

/// What the model is told a file outside the notes folder is called.
///
/// The folder it sits in and its own name, which is enough to tell two files of
/// one name apart in the reply and is what a person reading the pane would call
/// it. The rest of the path names the home directory and whoever the user works
/// for, and the model has no use for it (ADR-031 rule 2.5). A file with no named
/// parent, which is a file at the root of a volume, is its name alone.
fn outside_prompt_path(key: &str) -> String {
    let name = file_name_only(key);
    match Path::new(key).parent().and_then(Path::file_name) {
        Some(folder) => format!("{}/{}", folder.to_string_lossy(), name),
        None => name,
    }
}

/// Writes one proposal and records what became of it.
///
/// A note in the folder is written through the host, which holds
/// [`apply_permissions`]: the proposal's `before_hash` is what Writ last saw
/// the note hold, a note changed since the proposal was made is refused, and
/// the refusal writes the proposed text beside it as a dated copy rather than
/// over it (ADR-031 rule 4.3). The write carries no ignore stamp, so a tab
/// holding the note learns about it through the folder watcher the way it
/// learns about any other write it did not make (ADR-033).
///
/// A file outside the folder is written through `write_tab`, which is the tab's
/// own save path (ADR-040 section 13). The guarded facade belongs to the notes
/// folder: its history, its dated conflict copy and its watcher contract are
/// the folder's, and leaving a dated copy beside somebody else's repository is
/// not something the pane may do. The digest is compared here instead, and a
/// file that moved is refused with the proposed text left in the conversation.
#[allow(clippy::too_many_arguments)]
pub fn apply_proposal_inner(
    notes_root: &Path,
    open_notes: &dyn OpenNotes,
    writ_dir: &Path,
    host: &str,
    path: &str,
    new_content: &str,
    before_hash: &str,
    history: Option<&writ_storage::note_history::NoteHistoryStore>,
    write_tab: impl FnOnce(&str, &str, &[u8]) -> Result<(), String>,
) -> Result<ProposalOutcome, String> {
    let context = resolve_context_file(notes_root, open_notes, path)?;
    if let ContextScope::OpenTab { tab_id } = &context.scope {
        let tab_id = tab_id.clone();
        return apply_to_open_tab(
            notes_root,
            &context,
            writ_dir,
            host,
            path,
            new_content,
            before_hash,
            &tab_id,
            write_tab,
        );
    }
    let note_key = context.key;
    let Some(digest) = digest_from_hex(before_hash) else {
        return Err(format!("The recorded state of {note_key} is not readable."));
    };

    // Only the digest is handed over: the guard compares digests, never the
    // length or the modification time, neither of which the pane knew about the
    // text it showed.
    let applier = NoteHostImpl::open(notes_root, None, apply_permissions())
        .map_err(|_| format!("{note_key} was not written."))?
        .with_history(history);
    let outcome = applier.write_note(path, new_content, Some(digest), WriteOrigin::Chat);

    match outcome {
        Ok(written) => {
            // A write that moved nothing records no bytes: a line claiming a
            // length was written is a claim about a write that did not happen.
            record_proposal(
                writ_dir,
                host,
                "apply_proposal",
                &note_key,
                Decision::Allow,
                written.changed.then_some(written.bytes),
            );
            Ok(ProposalOutcome {
                path: note_key,
                hash: written.hash,
                bytes: written.bytes,
                changed: written.changed,
            })
        }
        Err(error) => {
            record_proposal(
                writ_dir,
                host,
                "apply_proposal",
                &note_key,
                Decision::Refuse,
                None,
            );
            Err(refusal(&note_key, &error))
        }
    }
}

/// What the card reads when the tab's own save path refused the write.
///
/// The save answers in stable codes, which are not sentences a card may show,
/// and a card that says only that nothing was written leaves a person with
/// nowhere to go. The two refusals a person can act on get their own sentence;
/// anything else says which file and stops, because a code invented for
/// another surface is not a sentence.
fn save_error_sentence(name: &str, error: &str) -> String {
    use crate::commands::buffer::{ERR_FILE_REMOVED_ON_DISK, ERR_NOTE_READ_ONLY};
    if error.starts_with(ERR_NOTE_READ_ONLY) {
        return format!("{name} is read-only and was not written.");
    }
    if error.starts_with(ERR_FILE_REMOVED_ON_DISK) {
        return format!("{name} is no longer there and was not written.");
    }
    format!("{name} was not written.")
}

/// Applies a proposal to a file that is only reachable because a tab has it
/// open.
///
/// The digest is compared here rather than by the guarded facade, and a file
/// that moved is refused with nothing written beside it: the proposed text is
/// in the conversation file, which is where a person gets it back from, and a
/// dated copy in somebody else's repository is not the pane's to leave.
///
/// `write_tab` is handed the bytes the digest was checked against, so what it
/// records as the tab's disk state is the state this apply verified rather than
/// whatever a second read would find.
#[allow(clippy::too_many_arguments)]
fn apply_to_open_tab(
    notes_root: &Path,
    context: &ContextFile,
    writ_dir: &Path,
    host: &str,
    path: &str,
    new_content: &str,
    before_hash: &str,
    tab_id: &str,
    write_tab: impl FnOnce(&str, &str, &[u8]) -> Result<(), String>,
) -> Result<ProposalOutcome, String> {
    let name = context_name(context);
    // A file that grew past the ceiling or stopped being text since the offer
    // was made is refused in the words the attach would have used.
    let current = read_context(notes_root, context, path)?;
    if current.hash != before_hash {
        record_proposal(
            writ_dir,
            host,
            "apply_proposal",
            &context.key,
            Decision::Refuse,
            None,
        );
        return Err(format!(
            "{name} changed since this was proposed and was not written."
        ));
    }

    // A write of the text the file already holds is skipped, which is what the
    // guarded facade does for a note and what makes `changed: false` mean the
    // same thing on both sides.
    let changed = current.text != new_content;
    if changed {
        // The verified bytes travel with the write. The digest has just passed
        // on these, and a second read taken inside the write is a read a change
        // Writ never saw can land in: recording that as the tab's disk state is
        // the one thing the buffer store's guard exists to catch.
        write_tab(tab_id, new_content, current.text.as_bytes()).inspect_err(|_| {
            record_proposal(
                writ_dir,
                host,
                "apply_proposal",
                &context.key,
                Decision::Refuse,
                None,
            );
        })?;
    }
    record_proposal(
        writ_dir,
        host,
        "apply_proposal",
        &context.key,
        Decision::Allow,
        changed.then_some(new_content.len() as u64),
    );
    Ok(ProposalOutcome {
        path: context.key.clone(),
        hash: writ_core::hash::sha256_hex(new_content.as_bytes()),
        bytes: new_content.len() as u64,
        changed,
    })
}

/// Tells the tab holding `file` that an applied proposal changed it.
///
/// The write carries no ignore stamp, because ADR-033 is right that the folder
/// watcher is the channel a write Writ did not make reaches a tab through. That
/// channel is slow and, on a loaded machine, late. This is the same event the
/// watcher would build, delivered by the write itself; recording the written
/// bytes as the tab's disk state in the same step is what makes the watcher's
/// own delivery silent at
/// [`writ_core::watcher::change_event::modification_is_news`], so the tab gets
/// one event rather than two.
///
/// A note nobody has open is told nothing, and answers `None`.
pub fn announce_applied_note(state: &AppState, file: &Path, bytes: &[u8]) -> Option<String> {
    let note_id = state.open_notes().note_at(file)?;
    announce_note_change(state, &note_id, file, bytes);
    Some(note_id)
}

/// The same notice, for a caller that already knows which tab holds the file.
///
/// A proposal applied to a file outside the notes folder took that tab's id
/// from the resolver rather than from the folder watch, and the watch is the
/// one thing that can be missing for such a file (see
/// [`crate::state::AppState::open_tabs`]). Splitting the lookup off is what
/// lets the tab be told either way.
pub fn announce_note_change(state: &AppState, note_id: &str, file: &Path, bytes: &[u8]) {
    state.record_disk_state_bytes(note_id, file, bytes);
    state
        .event_bus
        .emit(crate::watcher::open_files::open_note_modified(
            note_id,
            file,
            Some(bytes),
        ));
}

/// What the pane shows when a write does not happen.
///
/// The pane names notes by their folder-relative key everywhere else, and the
/// one thing a person needs from a refusal is where the text they were about to
/// apply went instead.
fn refusal(note_key: &str, error: &HostError) -> String {
    match error {
        HostError::Conflict { conflict_copy, .. } => match conflict_copy {
            Some(copy) => {
                let copy_name = Path::new(copy)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| copy.clone());
                format!("{note_key} changed since the offer was made. The proposed text is beside it in {copy_name}.")
            }
            None => format!(
                "{note_key} changed since the offer was made, and the proposed text could not be written beside it."
            ),
        },
        _ => format!("{note_key} was not written."),
    }
}

/// Records that a proposal was read and not applied.
///
/// Every proposal reaches the log, applied or not, so the record of what a
/// model asked for does not depend on the answer (ADR-031 rule 5.5). A path
/// the folder does not hold is still recorded, under the name it was given.
pub fn discard_proposal_inner(
    notes_root: &Path,
    open_notes: &dyn OpenNotes,
    writ_dir: &Path,
    host: &str,
    path: &str,
) {
    let note_key = resolve_context_file(notes_root, open_notes, path)
        .map(|context| context.key)
        .unwrap_or_else(|_| file_name_only(path));
    record_proposal(
        writ_dir,
        host,
        "discard_proposal",
        &note_key,
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
    note_key: &str,
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
    .with_path(note_key);
    if let Some(bytes) = bytes {
        record = record.with_bytes(bytes);
    }
    if let Err(error) = writ_storage::activity_log::append(writ_dir, &record) {
        tracing::warn!(error = %error, "the activity log did not take a chat record");
    }
}

/// Which reply a frame belongs to: the conversation it is keyed by, and the
/// send that produced it.
///
/// Both travel on every frame. The pane holds one entry per conversation and
/// replaces it on each send, so a frame from a request the conversation has
/// moved on from has to be recognisable as one.
#[derive(Clone, Copy)]
struct FrameIds<'a> {
    conversation_id: &'a str,
    request_id: &'a str,
}

fn text_frame(ids: FrameIds<'_>, kind: &str, text: Option<String>) -> WritFrontendEvent {
    chat_frame(
        ids,
        kind,
        ChatFrame {
            text,
            ..ChatFrame::default()
        },
    )
}

/// The frame that ends a reply, carrying what it proposed, what it lost,
/// whether it was cut off and which connection answered.
///
/// A dropped block is reported rather than swallowed: a proposal that vanishes
/// without a word reads as a broken feature, and the path the model named with
/// a reason is all a person needs to see which it was (ADR-031 rule 5.2).
fn done_frame(
    ids: FrameIds<'_>,
    parsed: ParsedProposals,
    identity: Option<RequestIdentity>,
    truncated: bool,
) -> WritFrontendEvent {
    chat_frame(
        ids,
        "done",
        ChatFrame {
            parsed,
            identity,
            truncated,
            ..ChatFrame::default()
        },
    )
}

/// The frame a failed reply ends on.
///
/// `text` carries the same sentence the frame does, so a pane that renders a
/// terminal frame's text needs no change to keep showing it; `error` is what a
/// recovery action is chosen from.
fn error_frame(ids: FrameIds<'_>, frame: ChatErrorFrame) -> WritFrontendEvent {
    let message = frame.message.clone();
    chat_frame(
        ids,
        "error",
        ChatFrame {
            text: Some(message),
            error: Some(frame),
            ..ChatFrame::default()
        },
    )
}

/// What a frame carries beyond its kind.
#[derive(Default)]
struct ChatFrame {
    text: Option<String>,
    parsed: ParsedProposals,
    identity: Option<RequestIdentity>,
    error: Option<ChatErrorFrame>,
    truncated: bool,
}

fn chat_frame(ids: FrameIds<'_>, kind: &str, frame: ChatFrame) -> WritFrontendEvent {
    WritFrontendEvent::AiChat {
        conversation_id: ids.conversation_id.to_string(),
        request_id: ids.request_id.to_string(),
        kind: kind.to_string(),
        text: frame.text,
        proposals: frame.parsed.proposals,
        identity: frame.identity,
        error: frame.error.map(Box::new),
        dropped: frame.parsed.dropped,
        truncated: frame.truncated,
    }
}

/// Sends one frame to the pane. A frame that cannot be delivered is a warning:
/// the reply is already in the file, and there is nothing a second attempt
/// would reach.
fn emit_to_pane(app: &AppHandle, event: WritFrontendEvent) {
    if let Err(error) = emit_event(app, event) {
        tracing::warn!(error = %error, "failed to emit chat frame");
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

/// The one line a refused request writes.
///
/// The status and, when the envelope named one of the six reasons Writ knows,
/// that reason's own token. Neither is the server's text: `code` can only be a
/// [`RejectCode`], and no other part of the body is read (ADR-031 rule 5.2 as
/// narrowed by the ADR-040 amendment of 2026-09-17).
fn log_rejected(status: u16, code: Option<RejectCode>) {
    tracing::warn!(
        status,
        code = code.map(RejectCode::as_str).unwrap_or("none"),
        "chat request rejected"
    );
}

/// How much of a refusal body is read before the rest is dropped.
///
/// An envelope is a few hundred bytes; this is the ceiling on what a host can
/// make Writ hold while looking for one of six words.
const MAX_REJECT_BODY_BYTES: usize = 8 * 1024;

/// The reason a refusal names, when it names one on the allowlist.
///
/// The body is read here and nowhere else, is bounded, and leaves this
/// function only as a [`RejectCode`].
async fn reject_code_of(response: reqwest::Response) -> Option<RejectCode> {
    let mut stream = response.bytes_stream();
    let mut body: Vec<u8> = Vec::new();
    while let Some(Ok(bytes)) = stream.next().await {
        body.extend_from_slice(&bytes);
        if body.len() >= MAX_REJECT_BODY_BYTES {
            body.truncate(MAX_REJECT_BODY_BYTES);
            break;
        }
    }
    chat::parse_reject_code(&String::from_utf8_lossy(&body))
}

/// What a request that never reached the host reads as.
///
/// A local runtime that refused the connection is the one case with a step
/// behind it, so it gets a typed failure naming the runtime and the port it
/// was expected on. Everything else keeps the sanitized transport line.
fn transport_frame(prepared: &PreparedChat, error: &reqwest::Error) -> ChatErrorFrame {
    if prepared.is_localhost && error.is_connect() {
        return ChatErrorFrame::from_error(
            &ChatError::LocalServerOffline {
                runtime: prepared.identity.provider.clone(),
                host_port: prepared.host_port.clone(),
            },
            &prepared.identity,
        );
    }
    ChatErrorFrame::untyped(
        "unreachable",
        super::ai::connection_error_message(error, prepared.is_localhost),
        &prepared.identity,
    )
}

/// One thing that happens during a stream.
enum ChatEvent {
    Chunk(String),
    /// The stream ended. `truncated` is set when it ended at the model's token
    /// ceiling, so what arrived is the start of a reply rather than all of it.
    Done {
        truncated: bool,
    },
    Error(ChatErrorFrame),
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
            on_event(ChatEvent::Error(transport_frame(prepared, &error)));
            return;
        }
    };

    let status = response.status();
    if !status.is_success() {
        let code = reject_code_of(response).await;
        log_rejected(status.as_u16(), code);
        on_event(ChatEvent::Error(ChatErrorFrame::from_error(
            &ChatError::ProviderRejected {
                provider: prepared.identity.provider.clone(),
                status: status.as_u16(),
                code,
            },
            &prepared.identity,
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
                on_event(ChatEvent::Error(ChatErrorFrame::untyped(
                    "stream_failed",
                    super::ai::sanitize_ai_error(&error.to_string()),
                    &prepared.identity,
                )));
                return;
            }
        };
        buf.extend_from_slice(&bytes);
        for line in super::ai::drain_complete_lines(&mut buf) {
            match chat::parse_delta(prepared.provider, &line) {
                Delta::Text(text) => on_event(ChatEvent::Chunk(text)),
                Delta::Done => {
                    on_event(ChatEvent::Done { truncated: false });
                    return;
                }
                // The ceiling ends the stream as surely as a stop does. The
                // flag is what lets the pane say the reply was cut off, rather
                // than leaving a half-written note to vanish as an
                // unterminated block.
                Delta::Truncated => {
                    on_event(ChatEvent::Done { truncated: true });
                    return;
                }
                // Nothing the server wrote is shown or logged: an error
                // frame's fields are response text, which can quote the
                // request the note went out in (rule 5.2, rule §1.7). The
                // host and the fact of the failure are the whole record.
                Delta::Failed => {
                    tracing::warn!(
                        host = %prepared.host,
                        "the model server ended the stream with an error frame"
                    );
                    on_event(ChatEvent::Error(ChatErrorFrame::untyped(
                        "stream_failed",
                        "The model server ended the reply.".to_string(),
                        &prepared.identity,
                    )));
                    return;
                }
                Delta::Ignore => {}
            }
        }
    }

    if cancel.load(Ordering::Relaxed) {
        return;
    }
    on_event(ChatEvent::Done { truncated: false });
}

/// What a reply has produced so far: everything the model wrote, and the part
/// of it the pane may see.
///
/// The two are not the same string. A `writ-proposal` fence and its body are
/// withheld from the pane and from the file, while `parse_proposals` still
/// reads them out of the whole reply on `done` (ADR-040 section 9). Holding
/// both here is what keeps the filter's output and the saved `content` one
/// thing rather than two that have to agree.
struct ReplyBuffer {
    raw: String,
    shown: String,
    filter: ProposalFilter,
}

impl ReplyBuffer {
    fn new() -> Self {
        Self {
            raw: String::new(),
            shown: String::new(),
            filter: ProposalFilter::new(),
        }
    }

    /// Takes one delta and hands back the text that may be shown now, which is
    /// empty while a proposal is being withheld.
    fn push(&mut self, delta: &str) -> String {
        self.raw.push_str(delta);
        let visible = self.filter.push(delta);
        self.shown.push_str(&visible);
        visible
    }

    /// Ends the stream, releasing a buffered prefix that never became a fence.
    fn finish(&mut self) -> String {
        let rest = self.filter.finish();
        self.shown.push_str(&rest);
        rest
    }
}

/// Ends the stream and sends what the filter was still holding as one last
/// `chunk`, before the frame that ends the reply.
///
/// A reply whose last line is a closing fence with no newline after it leaves
/// those backticks in the filter until the stream ends. They are part of the
/// saved reply, so the pane has to be handed them too; without this the pane
/// renders an unterminated fence until the conversation is reopened.
fn emit_tail<F: FnMut(WritFrontendEvent)>(
    emit: &mut F,
    ids: FrameIds<'_>,
    buffer: &mut ReplyBuffer,
) {
    let tail = buffer.finish();
    if !tail.is_empty() {
        emit(text_frame(ids, "chunk", Some(tail)));
    }
}

/// One reply, from the request to the turn it leaves in the file.
///
/// This is the whole of what a send spawns, the guard aside, so a test that
/// interrupts this interrupts what the app runs.
pub async fn run_reply(
    client: &reqwest::Client,
    prepared: &PreparedChat,
    cancel: &AtomicBool,
    store: &ChatStore,
    conversation_id: &str,
    request_id: &str,
    emit: impl FnMut(WritFrontendEvent),
) {
    let identity = prepared.identity.clone();
    stream_reply(
        client,
        prepared,
        cancel,
        FrameIds {
            conversation_id,
            request_id,
        },
        |shown, parsed, truncated| {
            record_reply(store, conversation_id, shown, parsed, truncated, &identity)
        },
        emit,
    )
    .await;
}

/// Streams one reply: every frame the pane sees and every write the file
/// takes, for one send.
///
/// The frames and the save are handed in rather than reached for, so the
/// sequence a reply produces — the chunks, the tail, the terminal frame, and
/// the save that comes before it — is the same code a test drives against a
/// stub host as the one a send runs.
async fn stream_reply(
    client: &reqwest::Client,
    prepared: &PreparedChat,
    cancel: &AtomicBool,
    ids: FrameIds<'_>,
    mut record: impl FnMut(&str, &ParsedProposals, bool),
    mut emit: impl FnMut(WritFrontendEvent),
) {
    let mut buffer = ReplyBuffer::new();
    let mut ended = false;
    run_chat_stream(client, prepared, cancel, |event| match event {
        ChatEvent::Chunk(text) => {
            let visible = buffer.push(&text);
            if !visible.is_empty() {
                emit(text_frame(ids, "chunk", Some(visible)));
            }
        }
        ChatEvent::Done { truncated } => {
            ended = true;
            emit_tail(&mut emit, ids, &mut buffer);
            let parsed = chat::parse_proposals(&buffer.raw, &prepared.context, truncated);
            record(&buffer.shown, &parsed, truncated);
            emit(done_frame(
                ids,
                parsed,
                Some(prepared.identity.clone()),
                truncated,
            ));
        }
        ChatEvent::Error(frame) => {
            ended = true;
            emit_tail(&mut emit, ids, &mut buffer);
            record(&buffer.shown, &ParsedProposals::default(), false);
            emit(error_frame(ids, frame));
        }
    })
    .await;

    // A stopped reply ends the stream with no terminal event of its own.
    // What arrived before the stop is the reply, so it is saved and the pane
    // is told to render what it has.
    if !ended {
        emit_tail(&mut emit, ids, &mut buffer);
        record(&buffer.shown, &ParsedProposals::default(), false);
        emit(text_frame(ids, "stopped", None));
    }
}

/// Appends what a stream produced to the stored conversation and saves it.
///
/// A reply that produced neither text nor a proposal appends nothing and still
/// saves, so the stamp records that the conversation was used. A store that
/// refuses is a warning and not an error: the reply is already on screen, and
/// there is nothing a person could do with a second message about it.
fn record_reply(
    store: &ChatStore,
    conversation_id: &str,
    shown: &str,
    parsed: &ParsedProposals,
    truncated: bool,
    identity: &RequestIdentity,
) {
    let reply = AssistantReply {
        proposals: parsed.proposals.iter().map(StoredProposal::from).collect(),
        dropped: parsed.dropped.clone(),
        truncated,
    };
    let empty = shown.is_empty()
        && reply.proposals.is_empty()
        && reply.dropped.is_empty()
        && !reply.truncated;
    let saved = store.load(conversation_id).and_then(|mut conversation| {
        if !empty {
            conversation.push_assistant(
                shown.to_string(),
                reply,
                Some(identity.clone()),
                ChatStore::now(),
            );
        }
        store.save(&conversation)
    });
    if let Err(error) = saved {
        tracing::warn!(error = %error, "the reply was not saved to its conversation");
    }
}

/// Records what became of one proposal.
///
/// A conversation the store no longer holds does not block the write that has
/// already happened; it is a warning, because the note on disk is the thing
/// that mattered and the record of it is in the activity log either way.
fn record_status(
    store: &ChatStore,
    conversation_id: &str,
    turn: usize,
    path: &str,
    status: ProposalStatus,
) {
    let saved = store.load(conversation_id).and_then(|mut conversation| {
        if !conversation.set_proposal_status(turn, path, status, ChatStore::now()) {
            tracing::warn!(turn, "no stored proposal answers to that turn");
        }
        store.save(&conversation)
    });
    if let Err(error) = saved {
        tracing::warn!(error = %error, "a proposal's status was not saved");
    }
}

/// Whether the document would still fit in a file once it is saved.
///
/// Measured on the bytes a save would write, which is what the store's own cap
/// is measured on, so a send refused here is a send the store would refuse
/// too (ADR-040 section 8).
fn fits_in_a_file(conversation: &Conversation) -> bool {
    serde_json::to_vec_pretty(conversation)
        .map(|bytes| bytes.len() <= MAX_CONVERSATION_BYTES)
        .unwrap_or(false)
}

/// The notes a turn carried, named rather than copied.
///
/// The bytes are the text the model was given, and the hash is the state a
/// proposal for that note is judged against. Neither is the note's text: the
/// note is on disk, and a second copy in the conversation would be a copy
/// ADR-028 forbids.
fn attachment_refs(context: &[AttachedNote]) -> Vec<AttachmentRef> {
    context
        .iter()
        .map(|note| AttachmentRef {
            path: note.path.clone(),
            bytes: note.text.len() as u64,
            hash: note.before_hash.clone(),
        })
        .collect()
}

/// Starts a streaming reply, appending the user's turn to the conversation
/// first.
///
/// `conversation_id` names the file the turns live in and keys the
/// `writ://ai-chat` frames; `request_id` names this send among the sends that
/// conversation has had, so a stop and a frame both say which reply they mean.
/// `truncate_to` cuts the conversation to that many turns before the new one is
/// appended, which is what retrying a turn and editing one both are.
///
/// The user's turn is saved after the request is resolved rather than before:
/// a send a switch, a consent or a missing key refuses never happened, and a
/// file that recorded it would grow a turn with no reply every time somebody
/// pressed Send against a connection that is not ready.
#[tauri::command]
pub async fn chat_send(
    app: AppHandle,
    conversation_id: String,
    text: String,
    context_paths: Vec<String>,
    truncate_to: Option<usize>,
    request_id: String,
) -> Result<ChatSendAccepted, String> {
    let cfg = chat_config(&app);
    if text.trim().is_empty() {
        return Err(ChatError::EmptyMessage.to_string());
    }
    // Two streams on one conversation are two tasks appending to one file, so
    // the conversation is claimed before anything is read, in one lock, and
    // the guard hands it back on every path out of here — a refusal below as
    // surely as the end of the stream.
    let (cancel, guard) = begin_request(&app.state::<ChatState>(), &conversation_id, &request_id)?;

    let store = chat_store(&app);
    let mut conversation = store.load(&conversation_id).map_err(missing_or)?;

    let context = {
        let state = app.state::<AppState>();
        read_attached_in(&state.notes_root(), &state.open_tabs(), &context_paths)?
    };
    let attached_bytes: usize = context.iter().map(|note| note.text.len()).sum();

    if let Some(len) = truncate_to {
        conversation.truncate(len, ChatStore::now());
    }
    conversation.push_user(text, attachment_refs(&context), ChatStore::now());
    conversation.provider = cfg.provider.clone();
    conversation.model = cfg.chat_model().to_string();
    if !fits_in_a_file(&conversation) {
        return Err(CONVERSATION_FULL.to_string());
    }

    let turns = conversation.request_turns();
    let catalog = app
        .state::<super::ai::AiState>()
        .live_catalog(&cfg.provider);
    let prepared = prepare_chat(&cfg, &turns, context, catalog.as_ref(), |account| {
        super::ai::key_for(&app, account)
    })
    .map_err(|error| error.to_string())?;

    store
        .save(&conversation)
        .map_err(|error| error.to_string())?;
    log_request(&prepared, attached_bytes, turns.len());

    let client = super::ai::build_client()?;

    let attached = prepared.context.clone();
    let accepted_identity = prepared.identity.clone();
    let task_app = app.clone();
    let task_id = conversation_id.clone();
    let task_request_id = request_id.clone();
    tauri::async_runtime::spawn(async move {
        // The conversation is this request's until the task ends, however it
        // ends: the guard is dropped by a return, by a panic in the stream or
        // in the save, and by the end of the reply alike.
        let _live = guard;
        run_reply(
            &client,
            &prepared,
            &cancel,
            &store,
            &task_id,
            &task_request_id,
            |event| emit_to_pane(&task_app, event),
        )
        .await;
    });

    Ok(ChatSendAccepted {
        conversation_id,
        attached,
        identity: accepted_identity,
        request_id,
    })
}

/// Signals a live reply to stop. Further deltas are dropped and no proposal is
/// read out of half a reply: the text already on screen is the reply, and the
/// task saves it and emits `stopped` so the pane can render it.
///
/// The stop names the request it means. A pane that sent, was answered and
/// sent again would otherwise stop the second reply with the first one's
/// button; `None` is the blunt form shutdown uses, which stops whatever that
/// conversation is running.
#[tauri::command]
pub fn chat_stop(chat: State<'_, ChatState>, conversation_id: String, request_id: Option<String>) {
    chat.cancel(&conversation_id, request_id.as_deref());
}

/// Writes a proposal the user applied, and records what became of it.
///
/// The status reaches the conversation whichever way the write went: a
/// proposal the guard refused is as decided as one it wrote, and a card that
/// still offered its two buttons after a refusal would offer a write that
/// cannot happen.
#[tauri::command]
pub fn chat_apply_proposal(
    app: AppHandle,
    conversation_id: String,
    turn: usize,
    path: String,
    new_content: String,
    before_hash: String,
) -> Result<ProposalOutcome, String> {
    let (notes_root, writ_dir) = {
        let state = app.state::<AppState>();
        (state.notes_root(), state.writ_dir.clone())
    };
    let state = app.state::<AppState>();
    let open_tabs = state.open_tabs();
    let context = resolve_context_file(&notes_root, &open_tabs, &path).ok();
    let write_app = app.clone();
    let write_file = context.as_ref().map(|context| context.file.clone());
    let write_name = context
        .as_ref()
        .map_or_else(|| file_name_only(&path), context_name);
    let outcome = apply_proposal_inner(
        &notes_root,
        &open_tabs,
        &writ_dir,
        &chat_host(&app),
        &path,
        &new_content,
        &before_hash,
        Some(&state.note_history),
        |tab_id, content, verified| {
            let state = write_app.state::<AppState>();
            // The apply has just read this file and found it holding what the
            // offer was made against, and those are the bytes handed here.
            // Recording them as the tab's disk state is what stops the buffer
            // store's own guard reading the write as one landing over a change
            // Writ never saw, which for a file outside the folder would leave a
            // dated copy beside it.
            if let Some(file) = write_file.as_ref() {
                state.record_disk_state_bytes(tab_id, file, verified);
            }
            crate::commands::buffer::save_buffer_content_inner(&state, tab_id, content)
                .map(|_| ())
                .map_err(|error| save_error_sentence(&write_name, &error))
        },
    );
    // Only a write that moved bytes has something to tell a tab. An apply of
    // the text the note already holds leaves the file exactly as the tab has
    // it, so an event about it would be a notice about nothing.
    if outcome.as_ref().is_ok_and(|applied| applied.changed) {
        if let Some(context) = context.as_ref() {
            let bytes = match context.scope {
                // The buffer store keeps the line ending the file keeps, so
                // what landed is not always the string that was offered. The
                // tab is told what the file holds.
                ContextScope::OpenTab { .. } => {
                    std::fs::read(&context.file).unwrap_or_else(|_| new_content.as_bytes().to_vec())
                }
                ContextScope::Notes => new_content.as_bytes().to_vec(),
            };
            match &context.scope {
                ContextScope::OpenTab { tab_id } => {
                    announce_note_change(&state, tab_id, &context.file, &bytes)
                }
                ContextScope::Notes => {
                    announce_applied_note(&state, &context.file, &bytes);
                }
            }
        }
    }
    let status = if outcome.is_ok() {
        ProposalStatus::Applied
    } else {
        ProposalStatus::Refused
    };
    record_status(&chat_store(&app), &conversation_id, turn, &path, status);
    announce_activity(&app);
    outcome
}

/// Records that a proposal was read and not applied.
#[tauri::command]
pub fn chat_discard_proposal(app: AppHandle, conversation_id: String, turn: usize, path: String) {
    let (notes_root, writ_dir) = {
        let state = app.state::<AppState>();
        (state.notes_root(), state.writ_dir.clone())
    };
    discard_proposal_inner(
        &notes_root,
        &app.state::<AppState>().open_tabs(),
        &writ_dir,
        &chat_host(&app),
        &path,
    );
    record_status(
        &chat_store(&app),
        &conversation_id,
        turn,
        &path,
        ProposalStatus::Discarded,
    );
    announce_activity(&app);
}

/// The host the pane is talking to, which is how the log names it.
fn chat_host(app: &AppHandle) -> String {
    let cfg = chat_config(app);
    polish::resolve_endpoint(&cfg.effective_base_url())
        .map(|target| target.host)
        .unwrap_or_default()
}

/// Tells any open activity view that the log grew.
fn announce_activity(app: &AppHandle) {
    if let Err(error) = emit_event(app, WritFrontendEvent::ActivityChanged {}) {
        tracing::warn!(error = %error, "failed to emit activity event");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::buffer::{ERR_FILE_REMOVED_ON_DISK, ERR_NOTE_READ_ONLY};
    use crate::watcher::open_files::NoOpenNotes;

    /// What `chat_send` asks before it appends a turn: a conversation whose
    /// reply is still arriving takes no second send, because two streams on
    /// one id are two tasks saving over each other's file and one cancel flag
    /// the pane can no longer reach.
    #[test]
    fn a_conversation_is_live_from_the_send_until_the_stream_ends() {
        let state = ChatState::default();
        let id = "0b7d6b7a-1111-4b6a-9d5e-000000000001";

        assert!(!state.is_live(id));
        let (cancel, guard) = begin_request(&state, id, "r-1").expect("the send was accepted");
        assert!(state.is_live(id));
        assert!(!state.is_live("0b7d6b7a-2222-4b6a-9d5e-000000000002"));

        // A stop asks the task to end; the reply is still arriving until it
        // has, so the conversation stays live until the task says so.
        assert!(state.cancel(id, Some("r-1")));
        assert!(cancel.load(Ordering::Relaxed));
        assert!(state.is_live(id));

        drop(guard);
        assert!(!state.is_live(id));
    }

    /// A connection pointed at a hand-typed endpoint, with the pane switched
    /// on or off. `custom` is the row whose base URL is read from the file, so
    /// a test can name an endpoint no table row carries.
    fn config(enabled: bool, base_url: &str, model: &str) -> AiConfig {
        AiConfig {
            provider: "custom".to_string(),
            base_url: base_url.to_string(),
            model: model.to_string(),
            chat: writ_core::config::AiChatConfig {
                enabled,
                model: String::new(),
                model_provider: String::new(),
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

    /// A fixed answer to "which tab is this file open in", standing in for the
    /// buffer rows [`crate::state::AppState::open_tabs`] reads.
    struct StubTabs(std::collections::HashMap<String, String>);

    impl StubTabs {
        /// Keys are built through the resolver's own canonicalisation, so the
        /// stub and `resolve_context_file` agree by construction rather than
        /// by coincidence: on macOS a temp dir is reached through `/var` and
        /// answered as `/private/var`.
        fn open(file: &Path, tab_id: &str) -> Self {
            let resolved =
                crate::security::resolve_for_containment(file).expect("the fixture file resolves");
            Self(std::collections::HashMap::from([(
                resolved,
                tab_id.to_string(),
            )]))
        }
    }

    impl OpenNotes for StubTabs {
        fn note_at(&self, path: &Path) -> Option<String> {
            self.0.get(&path.to_string_lossy().into_owned()).cloned()
        }
    }

    /// A notes folder and a file beside it that the folder does not hold.
    ///
    /// Returns the temp dir, the notes root, and the outside file's path as a
    /// caller would hand it in: absolute, and not through the root.
    fn notes_and_an_outside_file(contents: &[u8]) -> (tempfile::TempDir, PathBuf, String) {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let notes = dir.path().join("notes");
        let outside = dir.path().join("outside");
        std::fs::create_dir(&notes).expect("the notes folder");
        std::fs::create_dir(&outside).expect("the outside folder");
        let readme = outside.join("README.md");
        std::fs::write(&readme, contents).expect("write the outside file");
        let given = readme.to_string_lossy().into_owned();
        (dir, notes, given)
    }

    /// The path the resolver answers with, which is the key every chat surface
    /// names an outside file by.
    fn resolved_key(given: &str) -> String {
        crate::security::resolve_for_containment(Path::new(given)).expect("the file resolves")
    }

    #[test]
    fn outside_open_tab_is_readable_context() {
        let (_dir, notes, given) = notes_and_an_outside_file(b"the readme text\n");
        let tabs = StubTabs::open(Path::new(&given), "tab-1");

        let attached = read_attached_in(&notes, &tabs, std::slice::from_ref(&given))
            .expect("the open tab is context");
        assert_eq!(attached.len(), 1);
        assert_eq!(attached[0].path, resolved_key(&given));
        assert_eq!(attached[0].text, "the readme text\n");
        assert_eq!(
            attached[0].before_hash,
            writ_core::hash::sha256_hex(b"the readme text\n")
        );
    }

    #[test]
    fn a_save_refusal_says_why_the_file_was_not_written() {
        assert_eq!(
            save_error_sentence(
                "README.md",
                &format!("{ERR_NOTE_READ_ONLY}: note tab-1 is read-only")
            ),
            "README.md is read-only and was not written."
        );
        assert_eq!(
            save_error_sentence(
                "README.md",
                &format!("{ERR_FILE_REMOVED_ON_DISK}: note tab-1 has no file on disk any more")
            ),
            "README.md is no longer there and was not written."
        );
    }

    #[test]
    fn a_save_refusal_with_no_known_code_still_names_the_file() {
        assert_eq!(
            save_error_sentence("README.md", "ERR_PERMISSION_DENIED: the filesystem said no"),
            "README.md was not written."
        );
        assert_eq!(
            save_error_sentence("README.md", ""),
            "README.md was not written.",
            "a code the card has no sentence for still says which file, never the code"
        );
    }

    #[test]
    fn an_outside_open_tab_is_sent_under_its_folder_and_file_name() {
        let (_dir, notes, given) = notes_and_an_outside_file(b"the readme text\n");
        let tabs = StubTabs::open(Path::new(&given), "tab-1");

        let attached = read_attached_in(&notes, &tabs, std::slice::from_ref(&given))
            .expect("the open tab is context");
        assert_eq!(
            attached[0].prompt_path, "outside/README.md",
            "the model is told the folder and the file, never where that folder sits"
        );
        assert_eq!(
            attached[0].path,
            resolved_key(&given),
            "the key the apply writes to is still the whole path"
        );
    }

    #[test]
    fn a_file_with_no_named_parent_is_sent_under_its_name_alone() {
        assert_eq!(
            outside_prompt_path("/Users/someone/work/tessera/README.md"),
            "tessera/README.md"
        );
        assert_eq!(
            outside_prompt_path("/README.md"),
            "README.md",
            "a file at the root of a volume has no folder to name"
        );
    }

    #[test]
    fn a_note_in_the_folder_is_sent_under_its_key() {
        let (_dir, notes, _given) = notes_and_an_outside_file(b"the readme text\n");
        std::fs::write(notes.join("Launch.md"), "the note text\n").expect("write the note");

        let attached = read_attached_in(&notes, &NoOpenNotes, &["Launch.md".to_string()])
            .expect("the note is context");
        assert_eq!(attached[0].prompt_path, "Launch.md");
        assert_eq!(attached[0].path, "Launch.md");
    }

    #[test]
    fn outside_file_with_no_tab_is_refused() {
        let (_dir, notes, given) = notes_and_an_outside_file(b"the readme text\n");

        assert_eq!(
            read_attached_in(&notes, &NoOpenNotes, std::slice::from_ref(&given)),
            Err("README.md is not in the notes folder.".to_string())
        );
    }

    #[test]
    fn attached_sizes_report_an_outside_open_tab() {
        let (_dir, notes, given) = notes_and_an_outside_file(b"the readme text\n");
        let tabs = StubTabs::open(Path::new(&given), "tab-1");

        let sizes =
            attached_sizes_in(&notes, &tabs, std::slice::from_ref(&given)).expect("the sizes");
        assert_eq!(sizes.len(), 1);
        // The path is echoed exactly as it was asked about; the key is the
        // resolved absolute path the rest of the pane names the file by.
        assert_eq!(sizes[0].path, given);
        assert_eq!(sizes[0].key, resolved_key(&given));
        assert_eq!(sizes[0].bytes, "the readme text\n".len() as u64);
    }

    #[test]
    fn outside_open_tab_over_2mb_is_refused() {
        let big = vec![b'a'; (writ_core::notes::host::MAX_NOTE_BYTES + 1) as usize];
        let (_dir, notes, given) = notes_and_an_outside_file(&big);
        let tabs = StubTabs::open(Path::new(&given), "tab-1");

        assert_eq!(
            read_attached_in(&notes, &tabs, std::slice::from_ref(&given)),
            Err("README.md is too large to attach.".to_string())
        );
    }

    #[test]
    fn outside_open_tab_that_is_not_text_is_refused() {
        let (_dir, notes, given) = notes_and_an_outside_file(&[0xff, 0xfe, 0x00]);
        let tabs = StubTabs::open(Path::new(&given), "tab-1");

        assert_eq!(
            read_attached_in(&notes, &tabs, std::slice::from_ref(&given)),
            Err("README.md is not text.".to_string())
        );
    }

    #[test]
    fn the_write_is_handed_the_bytes_the_digest_was_checked_against() {
        let (_dir, notes, given) = notes_and_an_outside_file(b"the readme text\n");
        let writ = tempfile::TempDir::new().expect("temp dir");
        let tabs = StubTabs::open(Path::new(&given), "tab-1");
        let seen: std::cell::RefCell<Vec<u8>> = std::cell::RefCell::new(Vec::new());

        apply_proposal_inner(
            &notes,
            &tabs,
            writ.path(),
            "api.example.com",
            &given,
            "a tighter readme\n",
            &writ_core::hash::sha256_hex(b"the readme text\n"),
            None,
            |_tab_id, _content, verified| {
                *seen.borrow_mut() = verified.to_vec();
                Ok(())
            },
        )
        .expect("the proposal applies");

        assert_eq!(
            seen.into_inner(),
            b"the readme text\n".to_vec(),
            "the tab's disk state is recorded from the bytes the digest passed on, \
             never from a second read a write could land inside"
        );
    }

    #[test]
    fn proposal_applies_to_an_outside_open_tab_through_its_save_path() {
        let (_dir, notes, given) = notes_and_an_outside_file(b"the readme text\n");
        let writ = tempfile::TempDir::new().expect("temp dir");
        let tabs = StubTabs::open(Path::new(&given), "tab-1");
        let asked: std::cell::RefCell<Option<(String, String)>> = std::cell::RefCell::new(None);

        let outcome = apply_proposal_inner(
            &notes,
            &tabs,
            writ.path(),
            "api.example.com",
            &given,
            "a tighter readme\n",
            &writ_core::hash::sha256_hex(b"the readme text\n"),
            None,
            |tab_id, content, _verified| {
                *asked.borrow_mut() = Some((tab_id.to_string(), content.to_string()));
                Ok(())
            },
        )
        .expect("the proposal applies");

        assert!(outcome.changed);
        assert_eq!(outcome.path, resolved_key(&given));
        assert_eq!(
            asked.into_inner(),
            Some(("tab-1".to_string(), "a tighter readme\n".to_string()))
        );

        let recent = writ_storage::activity_log::read_recent(writ.path(), 1);
        assert_eq!(recent.len(), 1);
        assert_eq!(
            recent[0].path.as_deref(),
            Some(Path::new(&resolved_key(&given)))
        );
        assert_eq!(recent[0].decision, Decision::Allow);
    }

    #[test]
    fn proposal_for_a_moved_outside_file_is_refused() {
        let (_dir, notes, given) = notes_and_an_outside_file(b"the readme text\n");
        let writ = tempfile::TempDir::new().expect("temp dir");
        let tabs = StubTabs::open(Path::new(&given), "tab-1");
        let outside = Path::new(&given).parent().expect("the outside folder");

        let refusal = apply_proposal_inner(
            &notes,
            &tabs,
            writ.path(),
            "api.example.com",
            &given,
            "a tighter readme\n",
            &writ_core::hash::sha256_hex(b"somebody else's readme\n"),
            None,
            |_, _, _| panic!("the write ran"),
        );

        assert_eq!(
            refusal,
            Err("README.md changed since this was proposed and was not written.".to_string())
        );
        // Nothing was written beside it: a dated copy belongs to the notes
        // folder's guard and not next to somebody else's repository.
        assert_eq!(
            std::fs::read_dir(outside)
                .expect("the outside folder")
                .count(),
            1
        );
        let recent = writ_storage::activity_log::read_recent(writ.path(), 1);
        assert_eq!(recent[0].decision, Decision::Refuse);
    }

    #[test]
    fn traversal_outside_the_folder_stays_refused() {
        let (_dir, notes, given) = notes_and_an_outside_file(b"the readme text\n");
        let walked = notes
            .join("..")
            .join("outside")
            .join("README.md")
            .to_string_lossy()
            .into_owned();
        assert_ne!(walked, given);

        assert_eq!(
            read_attached_in(&notes, &NoOpenNotes, &[walked]),
            Err("README.md is not in the notes folder.".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_out_of_the_folder_stays_refused() {
        let (_dir, notes, given) = notes_and_an_outside_file(b"the readme text\n");
        std::os::unix::fs::symlink(&given, notes.join("Linked.md")).expect("the link");

        // The refusal names the link as it was given, not what it points at.
        assert_eq!(
            read_attached_in(&notes, &NoOpenNotes, &["Linked.md".to_string()]),
            Err("Linked.md is not in the notes folder.".to_string())
        );
    }

    #[test]
    fn an_outside_open_tab_is_still_refused_by_the_note_host() {
        let (_dir, notes, given) = notes_and_an_outside_file(b"the readme text\n");
        let tabs = StubTabs::open(Path::new(&given), "tab-1");
        assert_eq!(
            tabs.note_at(Path::new(&resolved_key(&given))).as_deref(),
            Some("tab-1")
        );

        // The boundary moved in the pane and nowhere else: the host answers a
        // file outside the folder the same way whether or not a tab holds it.
        let host = NoteHostImpl::open(&notes, None, context_permissions()).expect("the host");
        assert!(matches!(
            host.read_note(&given),
            Err(HostError::OutsideNotesFolder { .. })
        ));
    }

    #[test]
    fn a_path_the_folder_does_not_hold_has_no_key() {
        assert_eq!(
            relative_key(Path::new("/notes"), Path::new("/notes/Ideas/Launch.md")),
            Ok("Ideas/Launch.md".to_string())
        );
        // Two notes of the same name in different folders never collapse into
        // one key: a path the root does not prefix is refused instead.
        assert_eq!(
            relative_key(Path::new("/notes"), Path::new("/elsewhere/Launch.md")),
            Err("Launch.md is not in the notes folder.".to_string())
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_verbatim_root_holds_the_note_it_names() {
        // The two spellings Windows hands the two sides: the root as the app
        // carries it, and the file as `resolve_for_containment` answers with
        // the prefix dropped. Compared as they come, a note plainly in the
        // folder has no key and the pane loses the row.
        assert_eq!(
            relative_key(
                Path::new(r"\\?\C:\notes"),
                Path::new(r"C:\notes\Ideas\Launch.md")
            ),
            Ok("Ideas/Launch.md".to_string())
        );
    }

    #[test]
    fn a_note_that_vanished_and_a_folder_are_refused_in_their_own_words() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        std::fs::create_dir(dir.path().join("Archive")).expect("folder");
        assert_eq!(
            note_file_in(dir.path(), "Launch.md"),
            Err("This note is no longer there.".to_string())
        );
        assert_eq!(
            note_file_in(dir.path(), "Archive"),
            Err("This is not a note.".to_string())
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_refusal_names_a_windows_note_without_its_folder() {
        assert_eq!(
            outside_notes(r"C:\Users\someone\private\Secrets.md"),
            "Secrets.md is not in the notes folder."
        );
    }

    #[test]
    fn a_switch_that_is_off_refuses_before_anything_is_read() {
        let cfg = config(false, "http://localhost:11434/v1", "llama3");
        assert_eq!(
            prepare_chat(&cfg, &turns(), Vec::new(), None, no_key),
            Err(ChatError::Disabled)
        );
    }

    #[test]
    fn an_unconsented_hosted_host_is_refused_before_a_body_is_built() {
        let cfg = config(true, "https://api.example.com/v1", "some-model");
        assert_eq!(
            prepare_chat(&cfg, &turns(), Vec::new(), None, |_| panic!(
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
            prepare_chat(&cfg, &turns(), Vec::new(), None, no_key),
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
            prepare_chat(&cfg, &turns(), Vec::new(), None, no_key),
            Err(ChatError::EndpointNotAllowed)
        );
    }

    #[test]
    fn a_local_endpoint_needs_neither_consent_nor_a_key() {
        let cfg = config(true, "http://localhost:11434/v1", "llama3");
        let prepared = prepare_chat(&cfg, &turns(), Vec::new(), None, |_| {
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
            prepare_chat(&cfg, &turns(), Vec::new(), None, no_key),
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
            prepare_chat(&cfg, &blank, Vec::new(), None, no_key),
            Err(ChatError::EmptyMessage)
        );
    }

    #[test]
    fn the_table_decides_the_wire_and_the_endpoint() {
        let mut cfg = config(true, "", "claude-sonnet-5");
        cfg.provider = "anthropic".to_string();
        cfg.consented_hosts = vec!["api.anthropic.com".to_string()];
        let prepared = prepare_chat(&cfg, &turns(), Vec::new(), None, |_| Some("k".to_string()))
            .expect("prepared");
        assert_eq!(prepared.provider, Provider::Anthropic);
        assert_eq!(prepared.endpoint, "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn the_pane_sends_its_own_model_only_when_it_names_one() {
        let mut cfg = config(true, "http://localhost:11434/v1", "llama3");
        let prepared = prepare_chat(&cfg, &turns(), Vec::new(), None, no_key).expect("prepared");
        assert_eq!(prepared.body["model"], "llama3");

        // An override travels with the provider it was picked under.
        cfg.chat.model = "mistral".to_string();
        cfg.chat.model_provider = "custom".to_string();
        let prepared = prepare_chat(&cfg, &turns(), Vec::new(), None, no_key).expect("prepared");
        assert_eq!(prepared.body["model"], "mistral");
        assert_eq!(prepared.identity.model, "mistral");
        assert_eq!(prepared.identity.provider, "custom");

        // The same override under another provider is not sent.
        cfg.chat.model_provider = "ollama".to_string();
        let prepared = prepare_chat(&cfg, &turns(), Vec::new(), None, no_key).expect("prepared");
        assert_eq!(prepared.body["model"], "llama3");
        assert_eq!(prepared.identity.model, "llama3");
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
    fn the_key_is_read_under_the_connection_s_provider_id() {
        let mut cfg = config(true, "", "llama-3.3-70b-versatile");
        cfg.provider = "groq".to_string();
        cfg.consented_hosts = vec!["api.groq.com".to_string()];
        let prepared = prepare_chat(&cfg, &turns(), Vec::new(), None, |account| {
            assert_eq!(account, "groq", "the chat read another provider's key");
            Some("secret".to_string())
        })
        .expect("prepared");
        assert_eq!(prepared.api_key.as_deref(), Some("secret"));
        // The pane and the rewrite path now share one account, so the state
        // the settings row reports is the one the send reads.
        assert_eq!(
            endpoint_state_from(
                &cfg,
                super::AiKeyState {
                    is_set: true,
                    memory_only: false,
                }
            )
            .provider,
            "groq"
        );
    }
}

/// What a conversation gains from a send, a reply and a decision, against a
/// real store and a real notes folder.
#[cfg(test)]
mod conversation_tests {
    use super::*;
    use crate::watcher::open_files::NoOpenNotes;
    use writ_core::chat::Role;

    const ID: &str = "0b7d6b7a-4444-4b6a-9d5e-000000000004";

    /// A reply that says something, proposes one change, and says something
    /// after it. The body's lines are what must never reach the pane or the
    /// file.
    const REPLY_WITH_A_PROPOSAL: &str = "Here is a shorter opening.\n\n\
```writ-proposal path=\"Launch.md\" summary=\"Fold the intros\"\n\
one intro, folded\n\
and a second line\n\
```\n\n\
Tell me if that reads better.\n";

    fn store_in(dir: &Path) -> ChatStore {
        let store = ChatStore::new(dir);
        let mut conversation = Conversation::new(
            ID.to_string(),
            ChatStore::now(),
            "custom".to_string(),
            "a-model".to_string(),
        );
        conversation.push_user(
            "Tighten the opening.".to_string(),
            Vec::new(),
            ChatStore::now(),
        );
        store.save(&conversation).expect("seed the conversation");
        store
    }

    /// A notes folder holding one note, and the note as the request read it.
    fn notes_with(text: &str) -> (tempfile::TempDir, Vec<AttachedNote>) {
        let dir = tempfile::TempDir::new().expect("temp dir");
        std::fs::write(dir.path().join("Launch.md"), text).expect("write the note");
        let attached = read_attached_in(dir.path(), &NoOpenNotes, &["Launch.md".to_string()])
            .expect("attach the note");
        (dir, attached)
    }

    /// Feeds a reply through the buffer one character at a time, the way a
    /// stream arrives, and hands back the `chunk` frames the pane was sent.
    ///
    /// Only what the stream closure in `chat_send` passes to `emit_chat` lands
    /// in the string, the tail included, so text the filter releases and no
    /// frame carries reads here as a gap against `buffer.shown`.
    fn stream_through(buffer: &mut ReplyBuffer, reply: &str) -> String {
        let mut emitted = String::new();
        for character in reply.chars() {
            emitted.push_str(&buffer.push(&character.to_string()));
        }
        // What `emit_tail` sends before the terminal frame.
        emitted.push_str(&buffer.finish());
        emitted
    }

    #[test]
    fn an_attachment_names_the_note_and_carries_no_text() {
        let (_notes, attached) = notes_with("one intro\nand another intro\n");
        let refs = attachment_refs(&attached);

        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "Launch.md");
        assert_eq!(refs[0].bytes, attached[0].text.len() as u64);
        assert_eq!(refs[0].hash, attached[0].before_hash);
        let written = serde_json::to_string(&refs[0]).expect("serialize");
        assert!(!written.contains("one intro"), "got: {written}");
    }

    #[test]
    fn a_conversation_at_the_cap_takes_no_further_turn() {
        let mut conversation = Conversation::new(
            ID.to_string(),
            ChatStore::now(),
            "custom".to_string(),
            "a-model".to_string(),
        );
        assert!(fits_in_a_file(&conversation));

        conversation.push_user(
            "x".repeat(MAX_CONVERSATION_BYTES),
            Vec::new(),
            ChatStore::now(),
        );
        assert!(!fits_in_a_file(&conversation));
    }

    #[test]
    fn a_finished_reply_is_saved_with_the_fence_gone_and_its_proposal_pending() {
        let data = tempfile::TempDir::new().expect("temp dir");
        let store = store_in(data.path());
        let (_notes, attached) = notes_with("one intro\nand another intro\n");

        let mut buffer = ReplyBuffer::new();
        let emitted = stream_through(&mut buffer, REPLY_WITH_A_PROPOSAL);
        let parsed = chat::parse_proposals(&buffer.raw, &attached, false);
        record_reply(
            &store,
            ID,
            &buffer.shown,
            &parsed,
            false,
            &tests_support::test_identity(),
        );

        // What the stream handed the pane is what the file holds, and neither
        // carries a fence character or a line of the proposed text.
        assert_eq!(emitted, buffer.shown);
        let saved = store.load(ID).expect("load");
        assert_eq!(saved.turns.len(), 2);
        let reply = &saved.turns[1];
        assert_eq!(reply.role, Role::Assistant);
        assert!(!reply.content.contains('`'), "got: {}", reply.content);
        assert!(
            !reply.content.contains("one intro, folded"),
            "got: {}",
            reply.content
        );
        assert!(reply.content.contains("Here is a shorter opening."));
        assert!(reply.content.contains("Tell me if that reads better."));

        assert_eq!(reply.proposals.len(), 1);
        assert_eq!(reply.proposals[0].path, "Launch.md");
        assert_eq!(reply.proposals[0].summary, "Fold the intros");
        assert_eq!(reply.proposals[0].status, ProposalStatus::Pending);
        assert_eq!(
            reply.proposals[0].new_content,
            "one intro, folded\nand a second line\n"
        );
    }

    #[test]
    fn a_reply_that_failed_partway_keeps_what_arrived() {
        let data = tempfile::TempDir::new().expect("temp dir");
        let store = store_in(data.path());

        let mut buffer = ReplyBuffer::new();
        buffer.push("The first half of an answer");
        buffer.finish();
        record_reply(
            &store,
            ID,
            &buffer.shown,
            &ParsedProposals::default(),
            false,
            &tests_support::test_identity(),
        );

        let saved = store.load(ID).expect("load");
        assert_eq!(saved.turns.len(), 2);
        assert_eq!(saved.turns[1].content, "The first half of an answer");
        assert!(saved.turns[1].proposals.is_empty());
    }

    #[test]
    fn a_reply_that_failed_before_it_said_anything_appends_no_turn() {
        let data = tempfile::TempDir::new().expect("temp dir");
        let store = store_in(data.path());

        record_reply(
            &store,
            ID,
            "",
            &ParsedProposals::default(),
            false,
            &tests_support::test_identity(),
        );

        let saved = store.load(ID).expect("load");
        assert_eq!(saved.turns.len(), 1);
        assert_eq!(saved.turns[0].role, Role::User);
    }

    #[test]
    fn a_reply_ending_in_a_closing_fence_reaches_the_pane() {
        let mut buffer = ReplyBuffer::new();
        let emitted = stream_through(&mut buffer, "here\n\n```rust\nlet x = 1;\n```");

        // The last three bytes arrive with no newline after them, so the
        // filter is still holding them when the stream ends. Without them the
        // pane renders an unterminated fence.
        assert!(emitted.ends_with("```"), "got: {emitted}");
        assert_eq!(emitted, buffer.shown);
    }

    #[test]
    fn a_reply_ending_mid_fence_word_reaches_the_pane() {
        let mut buffer = ReplyBuffer::new();
        let emitted = stream_through(&mut buffer, "here\n\n```writ-propos");

        assert!(emitted.ends_with("```writ-propos"), "got: {emitted}");
        assert_eq!(emitted, buffer.shown);
    }

    #[test]
    fn a_stopped_reply_keeps_what_arrived_and_withholds_half_a_proposal() {
        let data = tempfile::TempDir::new().expect("temp dir");
        let store = store_in(data.path());

        let mut buffer = ReplyBuffer::new();
        stream_through(
            &mut buffer,
            "Here you go.\n```writ-proposal path=\"Launch.md\"\nhalf a not",
        );
        record_reply(
            &store,
            ID,
            &buffer.shown,
            &ParsedProposals::default(),
            false,
            &tests_support::test_identity(),
        );

        let saved = store.load(ID).expect("load");
        assert_eq!(saved.turns.len(), 2);
        assert_eq!(saved.turns[1].content, "Here you go.\n");
        assert!(saved.turns[1].proposals.is_empty());
    }

    #[test]
    fn a_reply_that_named_a_note_nobody_attached_records_the_drop() {
        let data = tempfile::TempDir::new().expect("temp dir");
        let store = store_in(data.path());
        let (_notes, attached) = notes_with("one intro\n");

        let mut buffer = ReplyBuffer::new();
        stream_through(
            &mut buffer,
            "Here you go.\n```writ-proposal path=\"Nope.md\"\nnew\n```\n",
        );
        let parsed = chat::parse_proposals(&buffer.raw, &attached, false);
        record_reply(
            &store,
            ID,
            &buffer.shown,
            &parsed,
            true,
            &tests_support::test_identity(),
        );

        let saved = store.load(ID).expect("load");
        let reply = &saved.turns[1];
        assert!(reply.proposals.is_empty());
        assert_eq!(reply.dropped.len(), 1);
        assert_eq!(reply.dropped[0].named, "Nope.md");
        assert_eq!(
            reply.dropped[0].reason,
            writ_core::chat::DropReason::UnknownNote
        );
        assert!(reply.truncated, "the turn forgot the reply was cut off");
    }

    #[test]
    fn a_decided_proposal_is_recorded_where_it_was_offered() {
        let data = tempfile::TempDir::new().expect("temp dir");
        let store = store_in(data.path());
        let (_notes, attached) = notes_with("one intro\nand another intro\n");
        let mut buffer = ReplyBuffer::new();
        stream_through(&mut buffer, REPLY_WITH_A_PROPOSAL);
        let parsed = chat::parse_proposals(&buffer.raw, &attached, false);
        record_reply(
            &store,
            ID,
            &buffer.shown,
            &parsed,
            false,
            &tests_support::test_identity(),
        );

        record_status(&store, ID, 1, "Launch.md", ProposalStatus::Applied);
        assert_eq!(
            store.load(ID).expect("load").turns[1].proposals[0].status,
            ProposalStatus::Applied
        );

        // A turn or a path that names nothing changes nothing and raises
        // nothing: the write it describes has already happened.
        record_status(&store, ID, 9, "Launch.md", ProposalStatus::Discarded);
        record_status(&store, ID, 1, "Other.md", ProposalStatus::Discarded);
        assert_eq!(
            store.load(ID).expect("load").turns[1].proposals[0].status,
            ProposalStatus::Applied
        );
    }

    #[test]
    fn a_pending_proposal_is_read_against_the_note_as_it_stands() {
        let (notes, attached) = notes_with("one intro\nand another intro\n");
        let mut conversation = Conversation::new(
            ID.to_string(),
            ChatStore::now(),
            "custom".to_string(),
            "a-model".to_string(),
        );
        conversation.push_assistant(
            "Here is a shorter opening.".to_string(),
            AssistantReply {
                proposals: vec![StoredProposal {
                    path: "Launch.md".to_string(),
                    summary: "Fold the intros".to_string(),
                    before_hash: attached[0].before_hash.clone(),
                    new_content: "one intro, folded\n".to_string(),
                    status: ProposalStatus::Pending,
                }],
                ..AssistantReply::default()
            },
            None,
            ChatStore::now(),
        );

        let fresh = conversation_dto(notes.path(), &NoOpenNotes, conversation.clone());
        let card = &fresh.turns[0].proposals[0];
        assert!(!card.stale);
        assert!(!card.hunks.is_empty());
        let removed: Vec<&str> = card.hunks[0]
            .lines
            .iter()
            .filter(|line| line.kind == writ_core::diff::LineKind::Removed)
            .map(|line| line.text.as_str())
            .collect();
        assert_eq!(removed, vec!["one intro", "and another intro"]);

        // The note moves on: the diff is still against what is on disk, and
        // the card says so.
        std::fs::write(notes.path().join("Launch.md"), "somebody else's opening\n")
            .expect("rewrite the note");
        let moved = conversation_dto(notes.path(), &NoOpenNotes, conversation.clone());
        assert!(moved.turns[0].proposals[0].stale);
        assert!(!moved.turns[0].proposals[0].hunks.is_empty());

        // The note is gone: nothing to compare against, and applying would be
        // refused.
        std::fs::remove_file(notes.path().join("Launch.md")).expect("remove the note");
        let gone = conversation_dto(notes.path(), &NoOpenNotes, conversation);
        assert!(gone.turns[0].proposals[0].stale);
        assert!(gone.turns[0].proposals[0].hunks.is_empty());
    }

    #[test]
    fn a_decided_proposal_carries_no_diff() {
        let (notes, attached) = notes_with("one intro\n");
        let mut conversation = Conversation::new(
            ID.to_string(),
            ChatStore::now(),
            "custom".to_string(),
            "a-model".to_string(),
        );
        conversation.push_assistant(
            "Done.".to_string(),
            AssistantReply {
                proposals: vec![StoredProposal {
                    path: "Launch.md".to_string(),
                    summary: "Fold the intros".to_string(),
                    before_hash: attached[0].before_hash.clone(),
                    new_content: "one intro, folded\n".to_string(),
                    status: ProposalStatus::Applied,
                }],
                ..AssistantReply::default()
            },
            None,
            ChatStore::now(),
        );

        let card =
            &conversation_dto(notes.path(), &NoOpenNotes, conversation).turns[0].proposals[0];
        assert!(card.hunks.is_empty());
        assert!(!card.stale);
        assert_eq!(card.status, ProposalStatus::Applied);
    }

    #[test]
    fn a_conversation_reads_as_the_pane_expects_it() {
        let (notes, _attached) = notes_with("one intro\n");
        let mut conversation = Conversation::new(
            ID.to_string(),
            "2026-09-15T10:00:00+00:00".to_string(),
            "anthropic".to_string(),
            "claude-sonnet-5".to_string(),
        );
        conversation.push_user(
            "Tighten the opening.".to_string(),
            vec![AttachmentRef {
                path: "Launch.md".to_string(),
                bytes: 10,
                hash: "abcd".to_string(),
            }],
            "2026-09-15T10:01:00+00:00".to_string(),
        );

        let written =
            serde_json::to_value(conversation_dto(notes.path(), &NoOpenNotes, conversation))
                .expect("serialize");
        assert_eq!(written["id"], ID);
        assert_eq!(written["title"], "Tighten the opening.");
        assert_eq!(written["provider"], "anthropic");
        assert_eq!(written["turns"][0]["role"], "user");
        assert_eq!(written["turns"][0]["attachments"][0]["path"], "Launch.md");
        assert_eq!(
            written["turns"][0]["proposals"].as_array().map(Vec::len),
            Some(0)
        );
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
                    ChatEvent::Done { truncated } => seen.push(format!("done:{truncated}")),
                    ChatEvent::Error(frame) => seen.push(format!("error:{}", frame.message)),
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
        assert_eq!(events.last().map(String::as_str), Some("done:false"));
    }

    #[test]
    fn a_reply_cut_off_at_the_ceiling_ends_as_done_and_says_so() {
        let (base, _seen) = spawn_mock(
            "HTTP/1.1 200 OK",
            "Content-Type: text/event-stream\r\nConnection: close\r\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"half an ans\"}}]}\n\n\
             data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n",
        );
        let prepared = prepared_for(&base, Provider::OpenAiCompatible, None);
        let events = run_against(&prepared, Arc::new(AtomicBool::new(false)));

        assert_eq!(
            events.first().map(String::as_str),
            Some("chunk:half an ans")
        );
        assert_eq!(
            events.last().map(String::as_str),
            Some("done:true"),
            "a reply that stopped at the ceiling read as a whole one"
        );
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

    /// Every frame of a reply says which send it came from, chunks and the
    /// terminal frame alike. The pane keeps one exchange per conversation and
    /// replaces it on each send, so a frame with no request id on it cannot be
    /// told apart from one belonging to the send before.
    #[test]
    fn every_frame_carries_the_request_id() {
        fn frames_of(prepared: &PreparedChat, cancel: bool) -> Vec<(String, String)> {
            let seen = Arc::new(Mutex::new(Vec::new()));
            let sink = seen.clone();
            tauri::async_runtime::block_on(async move {
                let client = super::super::ai::build_client().expect("client");
                stream_reply(
                    &client,
                    prepared,
                    &AtomicBool::new(cancel),
                    FrameIds {
                        conversation_id: CONVERSATION,
                        request_id: REQUEST,
                    },
                    |_, _, _| {},
                    |event| {
                        let WritFrontendEvent::AiChat {
                            conversation_id,
                            request_id,
                            kind,
                            ..
                        } = event
                        else {
                            panic!("a chat frame reached the pane as another event");
                        };
                        assert_eq!(conversation_id, CONVERSATION);
                        sink.lock().expect("frames").push((kind, request_id));
                    },
                )
                .await;
            });
            Arc::try_unwrap(seen)
                .expect("one reference")
                .into_inner()
                .expect("frames")
        }

        const CONVERSATION: &str = "0b7d6b7a-1111-4b6a-9d5e-000000000001";
        const REQUEST: &str = "1f2e3d4c-5b6a-4790-8123-456789abcdef";

        let (base, _seen) = spawn_mock(
            "HTTP/1.1 200 OK",
            "Content-Type: text/event-stream\r\nConnection: close\r\n",
            ANTHROPIC_STREAM,
        );
        let answered = frames_of(&prepared_for(&base, Provider::Anthropic, None), false);

        let (base, _seen) = spawn_mock(
            "HTTP/1.1 200 OK",
            "Content-Type: text/event-stream\r\nConnection: close\r\n",
            ANTHROPIC_STREAM,
        );
        let stopped = frames_of(&prepared_for(&base, Provider::Anthropic, None), true);

        let (base, _seen) = spawn_mock(
            "HTTP/1.1 429 Too Many Requests",
            "Content-Length: 21\r\nConnection: close\r\n",
            "{\"error\":\"slow down\"}",
        );
        let refused = frames_of(&prepared_for(&base, Provider::Anthropic, None), false);

        let kinds: Vec<&str> = answered
            .iter()
            .chain(stopped.iter())
            .chain(refused.iter())
            .map(|(kind, _)| kind.as_str())
            .collect();
        assert!(kinds.contains(&"chunk"), "got: {kinds:?}");
        assert_eq!(answered.last().map(|(kind, _)| kind.as_str()), Some("done"));
        assert_eq!(
            stopped.last().map(|(kind, _)| kind.as_str()),
            Some("stopped")
        );
        assert_eq!(refused.last().map(|(kind, _)| kind.as_str()), Some("error"));

        for (kind, request_id) in answered.iter().chain(&stopped).chain(&refused) {
            assert_eq!(request_id, REQUEST, "a {kind} frame named no request");
        }
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

        // A data folder the append cannot create, on every OS: a regular file
        // stands where a parent directory would have to be, so `create_dir_all`
        // fails on the component rather than on a permission the platform
        // happens to grant. An unwritable absolute path is not portable — the
        // root of a Windows drive is writable, the directory gets made, the
        // append succeeds and the WARN this asserts is never written.
        let unwritable = tempfile::TempDir::new().expect("temp dir");
        let blocker = unwritable.path().join("not-a-folder");
        std::fs::write(&blocker, b"a file, not a folder\n").expect("seed the blocker");
        let writ_dir = blocker.join("child");

        // A store under the same blocked folder, so the conversation writes
        // fail too and their warnings are written with a reply and a note key
        // in hand.
        let store = ChatStore::new(&writ_dir);
        let logs = captured_logs(|| {
            log_request(&prepared, NOTE_TEXT.len(), 1);
            log_rejected(401, Some(RejectCode::InvalidApiKey));
            record_proposal(
                &writ_dir,
                "127.0.0.1",
                "apply_proposal",
                "Ideas/Launch.md",
                Decision::Allow,
                Some(12),
            );
            record_reply(
                &store,
                "0b7d6b7a-5555-4b6a-9d5e-000000000005",
                RECORDED_REPLY,
                &ParsedProposals::default(),
                false,
                &test_identity(),
            );
            record_status(
                &store,
                "0b7d6b7a-5555-4b6a-9d5e-000000000005",
                0,
                "Ideas/Launch.md",
                writ_core::chat::ProposalStatus::Applied,
            );
        });
        // The blocker is still the thing that stopped the append: a change
        // that made `open_lock` fail somewhere else would still write the
        // warning below, and this test would pass for a reason it did not
        // arrange.
        assert!(
            blocker.is_file(),
            "the append was stopped by something other than the file in its path"
        );
        assert!(
            !writ_dir.exists(),
            "the data folder was created after all: {writ_dir:?}"
        );

        // The positive control comes first. Every negative assertion below is
        // of the form "this string is not in the buffer", which a buffer that
        // never filled satisfies for the wrong reason. Each of the three calls
        // above writes exactly one line, and all three have to be here before
        // the absence of anything else means a thing.
        assert!(
            logs.contains("sending a chat request"),
            "the request line did not reach the capture: {logs}"
        );
        assert!(
            logs.contains("chat request rejected"),
            "the rejection line did not reach the capture: {logs}"
        );
        assert!(
            logs.contains("the activity log did not take a chat record"),
            "the activity line did not reach the capture: {logs}"
        );
        assert!(
            logs.contains("the reply was not saved to its conversation"),
            "the conversation line did not reach the capture: {logs}"
        );
        assert!(
            logs.contains("a proposal's status was not saved"),
            "the status line did not reach the capture: {logs}"
        );

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
            !logs.contains(writ_core::chat::SYSTEM_PROMPT_HEAD),
            "the system prompt reached the log: {logs}"
        );
        assert!(logs.contains("127.0.0.1"), "the host is loggable: {logs}");
        assert!(logs.contains("401"), "the status is loggable: {logs}");

        // The stream's failure arm, driven end to end rather than described:
        // a host answers with an error frame it wrote every word of.
        let (base, _seen) = tests_support::spawn_mock(
            "HTTP/1.1 200 OK",
            "Content-Type: text/event-stream\r\nConnection: close\r\n",
            ANTHROPIC_ERROR_STREAM,
        );
        let streamed = prepared_for(&base, Provider::Anthropic, Some(SECRET_KEY));
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = seen.clone();
        let streaming = captured_logs(|| {
            let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
            tauri::async_runtime::block_on(async {
                let client = super::super::ai::build_client().expect("client");
                run_chat_stream(&client, &streamed, &cancel, |event| {
                    if let ChatEvent::Error(frame) = event {
                        sink.lock().expect("events").push(frame.message);
                    }
                })
                .await;
            });
        });
        assert!(
            !streaming.contains(SERVER_ERROR_TEXT),
            "the server's own words reached the log: {streaming}"
        );
        assert!(
            streaming.contains("the model server ended the stream with an error frame"),
            "the failure line did not reach the capture: {streaming}"
        );
        assert!(
            streaming.contains("127.0.0.1"),
            "the host is still loggable: {streaming}"
        );
        let shown = seen.lock().expect("events").clone();
        assert_eq!(
            shown,
            vec!["The model server ended the reply.".to_string()],
            "the pane is shown a fixed sentence and nothing the host wrote"
        );
    }
}

#[cfg(test)]
mod reject_stream_tests {
    use super::tests_support::*;
    use super::*;
    use std::sync::{Arc, Mutex};

    /// The body DeepSeek answers a model it does not serve with. Its
    /// `message` is the one string that must never reach a person or a log.
    const DEEPSEEK_400: &str = "{\"error\":{\"message\":\"Model Not Exist\",\"type\":\"invalid_request_error\",\"code\":\"invalid_request_error\"}}";
    const SERVER_SENTENCE: &str = "Model Not Exist";

    #[test]
    fn a_rejected_request_names_the_mapped_code_and_never_the_server_sentence() {
        let (base, _seen) = spawn_mock(
            "HTTP/1.1 400 Bad Request",
            "Content-Type: application/json\r\nConnection: close\r\n",
            DEEPSEEK_400,
        );
        let mut prepared = prepared_for(&base, Provider::OpenAiCompatible, Some(SECRET_KEY));
        prepared.identity.provider = "deepseek".to_string();
        prepared.identity.model = "qwen2.5-coder:0.5b".to_string();

        let seen = Arc::new(Mutex::new(Vec::<ChatErrorFrame>::new()));
        let sink = seen.clone();
        let logs = captured_logs(|| {
            let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
            tauri::async_runtime::block_on(async {
                let client = super::super::ai::build_client().expect("client");
                run_chat_stream(&client, &prepared, &cancel, |event| {
                    if let ChatEvent::Error(frame) = event {
                        sink.lock().expect("frames").push(frame);
                    }
                })
                .await;
            });
        });

        let frames = seen.lock().expect("frames").clone();
        assert_eq!(frames.len(), 1, "one terminal failure: {frames:?}");
        let frame = &frames[0];
        assert_eq!(frame.kind, "provider_rejected");
        assert_eq!(frame.status, Some(400));
        assert_eq!(frame.provider, "deepseek");
        assert_eq!(
            frame.model, "qwen2.5-coder:0.5b",
            "the refusal names the model that was refused"
        );
        // Writ's own sentence, built from the code the envelope named.
        assert_eq!(
            frame.message,
            "DeepSeek rejected the request (400): the request was not accepted."
        );
        assert!(
            !frame.message.contains(SERVER_SENTENCE),
            "the server's words reached the pane: {}",
            frame.message
        );

        // The positive control first: the line has to be in the capture before
        // its lack of the server's text means anything.
        assert!(
            logs.contains("chat request rejected"),
            "the rejection line did not reach the capture: {logs}"
        );
        assert!(logs.contains("400"), "the status is loggable: {logs}");
        assert!(
            logs.contains("invalid_request"),
            "the mapped reason is loggable: {logs}"
        );
        assert!(
            !logs.contains(SERVER_SENTENCE),
            "the server's words reached the log: {logs}"
        );
        assert!(!logs.contains(SECRET_KEY), "a key reached the log: {logs}");
    }

    #[test]
    fn a_status_with_no_reason_on_the_allowlist_is_the_bare_status() {
        let (base, _seen) = spawn_mock(
            "HTTP/1.1 503 Service Unavailable",
            "Content-Type: text/html\r\nConnection: close\r\n",
            "<html>ZZ-gateway-text-nobody-may-see</html>",
        );
        let mut prepared = prepared_for(&base, Provider::OpenAiCompatible, None);
        prepared.identity.provider = "groq".to_string();

        let seen = Arc::new(Mutex::new(Vec::<ChatErrorFrame>::new()));
        let sink = seen.clone();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        tauri::async_runtime::block_on(async {
            let client = super::super::ai::build_client().expect("client");
            run_chat_stream(&client, &prepared, &cancel, |event| {
                if let ChatEvent::Error(frame) = event {
                    sink.lock().expect("frames").push(frame);
                }
            })
            .await;
        });

        let frames = seen.lock().expect("frames").clone();
        assert_eq!(frames[0].message, "Groq rejected the request (503).");
        assert_eq!(frames[0].status, Some(503));
        assert!(!frames[0].message.contains("ZZ-gateway-text-nobody-may-see"));
    }

    #[test]
    fn a_local_runtime_that_is_not_running_says_so_by_name() {
        // Bind then drop to obtain a port with nothing listening.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);

        let mut prepared = prepared_for(
            &format!("http://127.0.0.1:{port}"),
            Provider::OpenAiCompatible,
            None,
        );
        prepared.identity.provider = "ollama".to_string();
        prepared.host_port = format!("127.0.0.1:{port}");

        let seen = Arc::new(Mutex::new(Vec::<ChatErrorFrame>::new()));
        let sink = seen.clone();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        tauri::async_runtime::block_on(async {
            let client = super::super::ai::build_client().expect("client");
            run_chat_stream(&client, &prepared, &cancel, |event| {
                if let ChatEvent::Error(frame) = event {
                    sink.lock().expect("frames").push(frame);
                }
            })
            .await;
        });

        let frames = seen.lock().expect("frames").clone();
        assert_eq!(frames[0].kind, "local_server_offline");
        assert_eq!(
            frames[0].message,
            format!("Ollama is not running at 127.0.0.1:{port}.")
        );
    }
}

#[cfg(test)]
mod stream_budget_tests {
    use super::tests_support::*;
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    const CONNECT: Duration = Duration::from_secs(2);

    fn frame(text: &str) -> String {
        format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"{text}\"}}}}]}}\n\n")
    }

    fn run(base: &str, read: Duration) -> Vec<String> {
        let prepared = prepared_for(base, Provider::OpenAiCompatible, None);
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = seen.clone();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        tauri::async_runtime::block_on(async {
            let client = super::super::ai::build_client_with(CONNECT, read).expect("client");
            run_chat_stream(&client, &prepared, &cancel, |event| {
                sink.lock().expect("events").push(match event {
                    ChatEvent::Chunk(text) => format!("chunk:{text}"),
                    ChatEvent::Done { truncated } => format!("done:{truncated}"),
                    ChatEvent::Error(frame) => format!("error:{}", frame.kind),
                })
            })
            .await;
        });
        let events = seen.lock().expect("events").clone();
        events
    }

    /// Eight pieces, each after a pause: the whole reply takes well over the
    /// read budget, and no pause comes near it. Every piece arrives and the
    /// reply ends as the server said it did.
    #[test]
    fn a_reply_that_keeps_arriving_is_never_cut_off_by_the_clock() {
        let pause = Duration::from_millis(120);
        let read = Duration::from_millis(400);
        let pieces: Vec<(&'static str, Duration)> = [
            "one ", "two ", "three ", "four ", "five ", "six ", "seven ", "eight",
        ]
        .into_iter()
        .map(|word| {
            (
                Box::leak(frame(word).into_boxed_str()) as &'static str,
                pause,
            )
        })
        .chain(std::iter::once(("data: [DONE]\n\n", Duration::ZERO)))
        .collect();
        let base = spawn_trickle(pieces);

        let events = run(&base, read);

        let text: String = events
            .iter()
            .filter_map(|event| event.strip_prefix("chunk:"))
            .collect();
        assert_eq!(text, "one two three four five six seven eight");
        assert_eq!(events.last().map(String::as_str), Some("done:false"));
    }

    /// One piece, then silence past the budget: the stream ends with an error
    /// frame and what arrived before the silence was delivered first.
    #[test]
    fn a_reply_that_goes_silent_past_the_budget_is_given_up() {
        let read = Duration::from_millis(300);
        let first: &'static str = Box::leak(frame("first").into_boxed_str());
        let base = spawn_trickle(vec![
            (first, Duration::from_millis(900)),
            ("data: [DONE]\n\n", Duration::ZERO),
        ]);

        let events = run(&base, read);

        assert_eq!(
            events,
            vec!["chunk:first".to_string(), "error:stream_failed".to_string()]
        );
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

    /// An error frame the host chose the wording of, served over the socket so
    /// the stream's own failure arm runs. Both of its fields carry the token,
    /// so a line that quoted either one fails the rule §1.7 test.
    pub const ANTHROPIC_ERROR_STREAM: &str =
        include_str!("../../../crates/writ-core/tests/fixtures/chat/anthropic-error.sse");

    /// The wording in that frame, which is response text and nothing else.
    pub const SERVER_ERROR_TEXT: &str = "ZZ-server-text-that-must-never-be-logged";

    /// The connection a test's reply is attributed to.
    pub fn test_identity() -> RequestIdentity {
        RequestIdentity {
            provider: "custom".to_string(),
            model: "a-model".to_string(),
            host: "127.0.0.1".to_string(),
        }
    }

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

    /// A server that writes its reply in pieces, pausing after each one.
    ///
    /// The pauses are the point: a client whose budget covers the whole
    /// request gives up on a reply that is still arriving, and one whose
    /// budget is silence gives up only on the pause that outlasts it.
    pub fn spawn_trickle(pieces: Vec<(&'static str, std::time::Duration)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 8192];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
                );
                let _ = stream.flush();
                for (piece, pause) in pieces {
                    let _ = stream.write_all(piece.as_bytes());
                    let _ = stream.flush();
                    std::thread::sleep(pause);
                }
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    /// A request aimed at a local mock, carrying a note, a turn and a key.
    pub fn prepared_for(base: &str, provider: Provider, key: Option<&str>) -> PreparedChat {
        let context = vec![AttachedNote {
            path: "Ideas/Launch.md".to_string(),
            prompt_path: "Ideas/Launch.md".to_string(),
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
                &chat::system_prompt(&context),
                &turns,
                &context,
            ),
            api_key: key.map(str::to_string),
            host: "127.0.0.1".to_string(),
            host_port: "127.0.0.1:0".to_string(),
            is_localhost: true,
            context,
            identity: RequestIdentity {
                provider: "custom".to_string(),
                model: "a-model".to_string(),
                host: "127.0.0.1".to_string(),
            },
        }
    }

    /// What `tracing` wrote while `run` ran.
    ///
    /// Through `preview::log_capture`, which is a `Layer` on the process-wide
    /// registry rather than a thread-local subscriber. `tracing` caches each
    /// callsite's interest globally and recomputes it from whatever the
    /// registering thread's default is, so a thread-local capture goes deaf to
    /// any callsite a parallel test touched first while holding no subscriber.
    /// A rule §1.7 assertion of the form `!logs.contains(SECRET)` would then
    /// pass because the line never arrived, which is why the test that uses
    /// this asserts what it expects to find before asserting what it must not.
    pub fn captured_logs(run: impl FnOnce()) -> String {
        crate::preview::log_capture::capture(run).1.join("\n")
    }
}
