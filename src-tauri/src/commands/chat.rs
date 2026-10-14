//! IPC surface for the chat pane.
//!
//! The pane's request, stream, context and apply are [`writ_ai::chat`]. These
//! commands read the config and the open tabs, hand them over, deliver each
//! [`PaneFrame`] as `writ://ai-chat`, and do the two things that need an open
//! tab's buffer: an apply to a file outside the notes folder goes through that
//! tab's save path, and a tab holding a file a proposal changed is told so.

use std::path::Path;

use tauri::{AppHandle, Manager, State};
use tokio_util::sync::CancellationToken;
use writ_ai::chat::{self, PaneFrame, TabLookup};
use writ_core::chat::{ChatError, ProposalStatus};
use writ_core::config::AiConfig;
use writ_core::polish;
use writ_storage::chat_store::{ChatStore, ConversationSummary};
use writ_storage::paths::file_name_only;

pub use writ_ai::chat::{
    apply_permissions, begin_request, chat_delete_inner, context_permissions, endpoint_state_from,
    needs_key_lookup, note_file_in, prepare_chat, stop_live_chats, AttachedSize, ChatEndpointState,
    ChatSendAccepted, ChatState, ConversationDto, LiveGuard, PreparedChat, ProposalDto,
    ProposalOutcome, TurnDto, CHAT_SHUTDOWN_BUDGET, REPLY_IN_FLIGHT,
};

use super::ai::AiKeyState;
use crate::events::{emit_event, WritFrontendEvent};
use crate::poison::recover_poison;
use crate::state::AppState;
use crate::watcher::open_files::OpenNotes;

/// The open tabs, as the chat module asks about them.
struct Tabs<'a>(&'a dyn OpenNotes);

impl TabLookup for Tabs<'_> {
    fn tab_at(&self, file: &Path) -> Option<String> {
        self.0.note_at(file)
    }
}

/// The frame the pane receives on `writ://ai-chat`.
fn pane_event(frame: PaneFrame) -> WritFrontendEvent {
    WritFrontendEvent::AiChat {
        conversation_id: frame.conversation_id,
        request_id: frame.request_id,
        kind: frame.kind,
        text: frame.text,
        proposals: frame.proposals,
        identity: frame.identity,
        error: frame.error.map(Box::new),
        dropped: frame.dropped,
        truncated: frame.truncated,
    }
}

/// One reply, from the request to the turn it leaves in the file, with each
/// frame handed over as the event the pane receives ([`chat::run_reply`]).
pub async fn run_reply(
    client: &reqwest::Client,
    prepared: &PreparedChat,
    cancel: &CancellationToken,
    store: &ChatStore,
    conversation_id: &str,
    request_id: &str,
    mut emit: impl FnMut(WritFrontendEvent),
) {
    chat::run_reply(
        client,
        prepared,
        cancel,
        store,
        conversation_id,
        request_id,
        |frame| emit(pane_event(frame)),
    )
    .await;
}

/// The sizes of the notes the call named ([`chat::attached_sizes_in`]).
pub fn attached_sizes_in(
    notes_root: &Path,
    open_notes: &dyn OpenNotes,
    paths: &[String],
) -> Result<Vec<AttachedSize>, String> {
    chat::attached_sizes_in(notes_root, &Tabs(open_notes), paths)
}

/// Reads the notes the call named, in the order it named them
/// ([`chat::read_attached_in`]).
pub fn read_attached_in(
    notes_root: &Path,
    open_notes: &dyn OpenNotes,
    paths: &[String],
) -> Result<Vec<writ_core::chat::AttachedNote>, String> {
    chat::read_attached_in(notes_root, &Tabs(open_notes), paths)
}

/// Writes one proposal and records what became of it
/// ([`chat::apply_proposal_inner`]).
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
    chat::apply_proposal_inner(
        notes_root,
        &Tabs(open_notes),
        writ_dir,
        host,
        path,
        new_content,
        before_hash,
        history,
        write_tab,
    )
}

/// Records that a proposal was read and not applied
/// ([`chat::discard_proposal_inner`]).
pub fn discard_proposal_inner(
    notes_root: &Path,
    open_notes: &dyn OpenNotes,
    writ_dir: &Path,
    host: &str,
    path: &str,
) {
    chat::discard_proposal_inner(notes_root, &Tabs(open_notes), writ_dir, host, path);
}

/// The stored conversation as the pane reads it ([`chat::conversation_dto`]).
fn conversation_dto(
    notes_root: &Path,
    open_notes: &dyn OpenNotes,
    conversation: writ_core::chat::Conversation,
) -> ConversationDto {
    chat::conversation_dto(notes_root, &Tabs(open_notes), conversation)
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
    let conversation = chat_store(&app).load(&id).map_err(chat::missing_or)?;
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
    let renamed = chat_store(&app)
        .rename(&id, &title)
        .map_err(chat::missing_or)?;
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

/// Renders one reply to the fragment the pane inserts into its own DOM.
///
/// The untrusted variant: a model's reply is untrusted input (ADR-031 rule
/// 4.1), so raw HTML is dropped rather than passed through, and the pane loads
/// neither the diagram runtime nor the math one. A panic in the renderer fails
/// this reply's render instead of the app (ADR-045).
#[tauri::command]
pub fn chat_render_reply(markdown: String) -> Result<String, String> {
    render_reply_with(&markdown, |markdown| {
        writ_render::render_markdown_fragment_untrusted(markdown).html
    })
}

fn render_reply_with(
    markdown: &str,
    render: impl FnOnce(&str) -> String,
) -> Result<String, String> {
    // The closure never returns an error of its own: an `Err` is a panic.
    writ_core::preview::render_contained(|| Ok(render(markdown)))
        .map_err(|_| writ_core::preview::RENDERER_PANICKED.to_string())
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
    // The pane's write is not the editor's, so an opted-in file's scripts go
    // with it (ADR-043). An open-tab apply saves through the editor's path,
    // which carries the opt-in over its own write; this is what takes it back.
    crate::commands::preview::lapse_scripts_opt_in_now(state, note_id);
    state
        .event_bus
        .emit(crate::watcher::open_files::open_note_modified(
            note_id,
            file,
            Some(bytes),
            crate::watcher::moves::encoding_of_note(state, note_id),
        ));
}

/// Sends one frame to the pane. A frame that cannot be delivered is a warning:
/// the reply is already in the file, and there is nothing a second attempt
/// would reach.
fn emit_to_pane(app: &AppHandle, event: WritFrontendEvent) {
    if let Err(error) = emit_event(app, event) {
        tracing::warn!(error = %error, "failed to emit chat frame");
    }
}

/// Starts a streaming reply, appending the user's turn to the conversation
/// first ([`chat::prepare_send`]).
///
/// `conversation_id` names the file the turns live in and keys the
/// `writ://ai-chat` frames; `request_id` names this send among the sends that
/// conversation has had, so a stop and a frame both say which reply they mean.
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
    let catalog = app
        .state::<super::ai::AiState>()
        .live_catalog(&cfg.provider);
    let prepared = {
        let state = app.state::<AppState>();
        let open_tabs = state.open_tabs();
        chat::prepare_send(
            &cfg,
            &store,
            &state.notes_root(),
            &Tabs(&open_tabs),
            catalog.as_ref(),
            |account| super::ai::key_for(&app, account),
            &conversation_id,
            text,
            &context_paths,
            truncate_to,
        )?
    };

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
        chat::run_reply(
            &client,
            &prepared,
            &cancel,
            &store,
            &task_id,
            &task_request_id,
            |frame| emit_to_pane(&task_app, pane_event(frame)),
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
    let context = chat::resolve_context_file(&notes_root, &Tabs(&open_tabs), &path).ok();
    let write_app = app.clone();
    let write_file = context.as_ref().map(|context| context.file.clone());
    let write_name = context
        .as_ref()
        .map_or_else(|| file_name_only(&path), chat::context_name);
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
                chat::ContextScope::OpenTab { .. } => {
                    std::fs::read(&context.file).unwrap_or_else(|_| new_content.as_bytes().to_vec())
                }
                chat::ContextScope::Notes => new_content.as_bytes().to_vec(),
            };
            match &context.scope {
                chat::ContextScope::OpenTab { tab_id } => {
                    announce_note_change(&state, tab_id, &context.file, &bytes)
                }
                chat::ContextScope::Notes => {
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
    chat::record_status(&chat_store(&app), &conversation_id, turn, &path, status);
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
    chat::record_status(
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
}

#[cfg(test)]
mod render_reply_tests {
    use super::*;

    #[test]
    fn a_reply_renders_to_its_fragment() {
        assert_eq!(
            chat_render_reply("**bold**".to_string()).unwrap().trim(),
            "<p><strong>bold</strong></p>"
        );
    }

    #[test]
    fn a_renderer_panic_fails_the_reply_without_quoting_it() {
        let failed = render_reply_with("a private reply", |text| panic!("renderer bug on {text}"));

        assert_eq!(
            failed,
            Err(writ_core::preview::RENDERER_PANICKED.to_string())
        );
    }
}
