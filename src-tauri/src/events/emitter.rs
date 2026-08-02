use serde::Serialize;
use tauri::{AppHandle, Emitter, EventTarget};
use writ_core::update::UpdatePhase;

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

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "payload")]
pub enum WritFrontendEvent {
    #[serde(rename = "buffer:opened")]
    BufferOpened { id: String, title: String },

    #[serde(rename = "pending:opens")]
    PendingOpens { paths: Vec<String> },

    #[serde(rename = "files:dropped")]
    FilesDropped { paths: Vec<String> },

    #[serde(rename = "window:shown")]
    WindowShown { rust_elapsed_us: u64 },

    #[serde(rename = "config:changed")]
    ConfigChanged { keys: Vec<String> },

    #[serde(rename = "buffer:external")]
    BufferExternal { buffer_id: String, change: String },

    #[serde(rename = "menu:action")]
    MenuAction { action: String },

    #[serde(rename = "workspace:changed")]
    WorkspaceChanged { path: String, removed: bool },

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

    #[serde(rename = "preview:rendered")]
    PreviewRendered {
        buffer_id: String,
        window_id: u64,
        used_fallback_stylesheet: bool,
        parser_warnings: Vec<String>,
    },

    #[serde(rename = "preview:error")]
    PreviewError {
        buffer_id: String,
        window_id: u64,
        message: String,
    },

    #[serde(rename = "preview:layout_changed")]
    LayoutChanged {
        buffer_id: String,
        window_id: u64,
        layout: String,
        ratio: Option<f32>,
    },

    #[serde(rename = "titlebar:maximize-hit")]
    CaptionMaximizeHit { phase: CaptionHitPhase },
}

fn event_name(event: &WritFrontendEvent) -> &'static str {
    match event {
        WritFrontendEvent::BufferOpened { .. } => "writ://buffer-opened",
        WritFrontendEvent::PendingOpens { .. } => "writ://pending-opens",
        WritFrontendEvent::FilesDropped { .. } => "writ://files-dropped",
        WritFrontendEvent::WindowShown { .. } => "writ://window-shown",
        WritFrontendEvent::ConfigChanged { .. } => "writ://config-changed",
        WritFrontendEvent::BufferExternal { .. } => "writ://buffer-external",
        WritFrontendEvent::MenuAction { .. } => "writ://menu-action",
        WritFrontendEvent::InboxFileArrived { .. } => "writ://inbox-file-arrived",
        WritFrontendEvent::WorkspaceChanged { .. } => "writ://workspace-changed",
        WritFrontendEvent::UpdateStatus(..) => "writ://update-status",
        WritFrontendEvent::AiRewrite { .. } => "writ://ai-rewrite",
        WritFrontendEvent::PreviewRendered { .. } => "writ://preview-rendered",
        WritFrontendEvent::PreviewError { .. } => "writ://preview-error",
        WritFrontendEvent::LayoutChanged { .. } => "writ://preview-layout-changed",
        WritFrontendEvent::CaptionMaximizeHit { .. } => "writ://titlebar-maximize-hit",
    }
}

pub fn emit_event(app: &AppHandle, event: WritFrontendEvent) -> Result<(), String> {
    app.emit(event_name(&event), &event)
        .map_err(|e| e.to_string())
}

/// Emits to the main webview only, for events that describe that window's own
/// chrome and would be meaningless anywhere else.
pub fn emit_event_to_main(app: &AppHandle, event: WritFrontendEvent) -> Result<(), String> {
    app.emit_to(
        EventTarget::webview_window("main"),
        event_name(&event),
        &event,
    )
    .map_err(|e| e.to_string())
}
