use jotra_core::update::UpdatePhase;
use serde::Serialize;
use tauri::{AppHandle, Emitter, EventTarget};

/// Pointer phases the snap-layout overlay reports for the maximize button. The
/// overlay sits above the webview, so the button never sees the real mouse
/// events and takes its hover and press state from these instead.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptionHitPhase {
    Enter,
    Leave,
    Press,
    Click,
}

/// Where a drop landed, in the CSS pixels the webview lays out in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DropPoint {
    /// From the left edge of the webview.
    pub x: f64,
    /// From the top edge of the webview.
    pub y: f64,
}

/// Where a note's download has got to.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteDownloadState {
    /// The provider has been asked for the bytes.
    Started,
    /// The bytes are on this machine.
    Done,
    /// The provider could not produce the file.
    Failed,
    /// The person stopped waiting.
    Cancelled,
    /// The wait ran out with the file still not here.
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "payload")]
pub enum JotraFrontendEvent {
    #[serde(rename = "pending:opens")]
    PendingOpens {
        paths: Vec<String>,
        /// The 1-based line to show for a path, keyed by the path as `paths`
        /// spells it.
        #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        lines: std::collections::BTreeMap<String, u32>,
    },

    #[serde(rename = "files:dropped")]
    FilesDropped {
        paths: Vec<String>,
        /// Where the pointer was, when the platform reported it.
        #[serde(skip_serializing_if = "Option::is_none")]
        position: Option<DropPoint>,
    },

    /// A `jotra://new` link asked for a new tab, called `name` when it gave one
    /// (ADR-052). The tab reaches a file on its first keystroke.
    #[serde(rename = "link:new")]
    LinkNew { name: Option<String> },

    /// A program allowed to see the open file asked which one it is (ADR-053).
    /// The main window answers with `context_answer` and this `token`.
    #[serde(rename = "context:asked")]
    ContextAsked { token: String },

    /// A program's write waits for the person to apply or discard it
    /// (ADR-053). The main window answers with `review_answer` and the token.
    #[serde(rename = "review:asked")]
    ReviewAsked(crate::write_review::ShownReview),

    #[serde(rename = "window:shown")]
    WindowShown { rust_elapsed_us: u64 },

    #[serde(rename = "config:changed")]
    ConfigChanged { keys: Vec<String> },

    /// An open note's file changed outside Jotra. `bufferId` names the tab,
    /// `diskHash` is what the file holds now, and `newPath` is where it went
    /// for a change that is a move.
    #[serde(rename = "buffer:external")]
    #[serde(rename_all = "camelCase")]
    BufferExternal {
        buffer_id: String,
        path: String,
        change: String,
        new_path: Option<String>,
        disk_hash: Option<String>,
        writer: Option<jotra_core::notes::Writer>,
    },

    #[serde(rename = "menu:action")]
    MenuAction { action: String },

    #[serde(rename = "workspace:changed")]
    WorkspaceChanged { path: String, removed: bool },

    #[serde(rename = "workspace:swept")]
    WorkspaceSwept { root: String },

    #[serde(rename = "notes:changed")]
    NotesChanged { path: String, removed: bool },

    #[serde(rename = "notes:swept")]
    NotesSwept { root: String },

    /// Jotra wrote rows of the notes index itself; `paths` names the files
    /// whose rows changed, or the notes folder for a walk or a move.
    #[serde(rename = "notes:indexed")]
    NotesIndexed { paths: Vec<String> },

    #[serde(rename = "inbox:file-arrived")]
    InboxFileArrived { path: String },

    #[serde(rename = "update:status")]
    UpdateStatus(UpdatePhase),

    #[serde(rename = "ai:rewrite")]
    AiRewrite {
        request_id: String,
        kind: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },

    /// One frame of a chat reply, or its terminal state: `chunk`, `done`,
    /// `stopped` or `error`. `proposals` is carried only by the `done` frame
    /// and holds whole-note text the user has not applied to anything
    /// (ADR-031 rule 4.3). `dropped` and `truncated` are carried there too:
    /// they say why a reply offered less than it looked like it would, which
    /// is the difference between a broken feature and a model that named the
    /// wrong note.
    #[serde(rename = "ai:chat")]
    AiChat {
        conversation_id: String,
        /// The send this frame belongs to. Always present: a frame from a
        /// request its conversation has moved on from is recognisable only by
        /// this.
        request_id: String,
        kind: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        proposals: Vec<jotra_core::chat::Proposal>,
        /// The connection the reply came from, carried by `done`.
        #[serde(skip_serializing_if = "Option::is_none")]
        identity: Option<jotra_core::chat::RequestIdentity>,
        /// The typed failure, carried by `error`. Its `message` is the same
        /// sentence `text` holds.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<Box<jotra_core::chat::ChatErrorFrame>>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        dropped: Vec<jotra_core::chat::DroppedProposal>,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        truncated: bool,
    },

    #[serde(rename = "note:download")]
    NoteDownload {
        path: String,
        state: NoteDownloadState,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },

    /// Empty struct rather than a unit variant: under `tag`/`content` a unit
    /// variant serialises without the `payload` key the frontend unwraps.
    #[serde(rename = "quit:flush")]
    FlushBeforeQuit {},

    #[serde(rename = "titlebar:maximize-hit")]
    CaptionMaximizeHit { phase: CaptionHitPhase },

    /// The activity log changed by the app's own hand. The `jotra mcp` process
    /// cannot emit, so this is not every change: the panel also re-reads on
    /// open and while it is on screen.
    ///
    /// Empty struct rather than a unit variant, for the reason
    /// [`JotraFrontendEvent::FlushBeforeQuit`] gives.
    #[serde(rename = "activity:changed")]
    ActivityChanged {},

    #[serde(rename = "mcp:pending")]
    McpPending {
        waiting: Vec<jotra_core::activity::PendingClient>,
    },

    /// A program called while Connected programs was off: the record the
    /// server wrote, so the app can offer to turn it on.
    #[serde(rename = "mcp:off-call")]
    McpOffCall { call: jotra_core::activity::OffCall },

    /// Whether the OS gave Jotra the chord that shows and hides the window.
    /// Raised at startup and again on every rebind, so the shortcut editor can
    /// say a chord is taken rather than leaving a dead key.
    #[serde(rename = "hotkey:status")]
    HotkeyStatus(crate::hotkey::GlobalHotkeyStatus),

    /// [`Self::HotkeyStatus`] for the chord that opens the quick capture note
    /// (ADR-055).
    #[serde(rename = "hotkey:capture-status")]
    CaptureHotkeyStatus(crate::hotkey::GlobalHotkeyStatus),
}

fn event_name(event: &JotraFrontendEvent) -> &'static str {
    match event {
        JotraFrontendEvent::PendingOpens { .. } => "jotra://pending-opens",
        JotraFrontendEvent::FilesDropped { .. } => "jotra://files-dropped",
        JotraFrontendEvent::LinkNew { .. } => "jotra://link-new",
        JotraFrontendEvent::ContextAsked { .. } => "jotra://context-asked",
        JotraFrontendEvent::ReviewAsked(..) => "jotra://review-asked",
        JotraFrontendEvent::WindowShown { .. } => "jotra://window-shown",
        JotraFrontendEvent::ConfigChanged { .. } => "jotra://config-changed",
        JotraFrontendEvent::BufferExternal { .. } => "jotra://buffer-external",
        JotraFrontendEvent::MenuAction { .. } => "jotra://menu-action",
        JotraFrontendEvent::InboxFileArrived { .. } => "jotra://inbox-file-arrived",
        JotraFrontendEvent::WorkspaceChanged { .. } => "jotra://workspace-changed",
        JotraFrontendEvent::WorkspaceSwept { .. } => "jotra://workspace-swept",
        JotraFrontendEvent::NotesChanged { .. } => "jotra://notes-changed",
        JotraFrontendEvent::NotesSwept { .. } => "jotra://notes-swept",
        JotraFrontendEvent::NotesIndexed { .. } => "jotra://notes-indexed",
        JotraFrontendEvent::UpdateStatus(..) => "jotra://update-status",
        JotraFrontendEvent::AiRewrite { .. } => "jotra://ai-rewrite",
        JotraFrontendEvent::AiChat { .. } => "jotra://ai-chat",
        JotraFrontendEvent::NoteDownload { .. } => "jotra://note-download",
        JotraFrontendEvent::FlushBeforeQuit { .. } => "jotra://flush-before-quit",
        JotraFrontendEvent::CaptionMaximizeHit { .. } => "jotra://titlebar-maximize-hit",
        JotraFrontendEvent::ActivityChanged { .. } => "jotra://activity",
        JotraFrontendEvent::McpPending { .. } => "jotra://mcp-pending",
        JotraFrontendEvent::McpOffCall { .. } => "jotra://mcp-off-call",
        JotraFrontendEvent::HotkeyStatus(..) => "jotra://hotkey-status",
        JotraFrontendEvent::CaptureHotkeyStatus(..) => "jotra://hotkey-capture-status",
    }
}

pub fn emit_event(app: &AppHandle, event: JotraFrontendEvent) -> Result<(), String> {
    app.emit(event_name(&event), &event)
        .map_err(|e| e.to_string())
}

/// Emits to the main webview only, for events that describe that window's own
/// chrome and would be meaningless anywhere else.
pub fn emit_event_to_main(app: &AppHandle, event: JotraFrontendEvent) -> Result<(), String> {
    app.emit_to(
        EventTarget::webview_window("main"),
        event_name(&event),
        &event,
    )
    .map_err(|e| e.to_string())
}
