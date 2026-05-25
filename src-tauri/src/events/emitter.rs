use serde::Serialize;
use tauri::{AppHandle, Emitter};
use writ_core::update::UpdatePhase;

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

    #[serde(rename = "recovery:dirty")]
    RecoveryDirty {
        snapshot_id: String,
        buffer_count: u32,
    },

    #[serde(rename = "menu:action")]
    MenuAction { action: String },

    #[serde(rename = "update:status")]
    UpdateStatus(UpdatePhase),
}

pub fn emit_event(app: &AppHandle, event: WritFrontendEvent) -> Result<(), String> {
    let event_name = match &event {
        WritFrontendEvent::BufferOpened { .. } => "writ://buffer-opened",
        WritFrontendEvent::PendingOpens { .. } => "writ://pending-opens",
        WritFrontendEvent::FilesDropped { .. } => "writ://files-dropped",
        WritFrontendEvent::WindowShown { .. } => "writ://window-shown",
        WritFrontendEvent::ConfigChanged { .. } => "writ://config-changed",
        WritFrontendEvent::BufferExternal { .. } => "writ://buffer-external",
        WritFrontendEvent::RecoveryDirty { .. } => "writ://recovery-dirty",
        WritFrontendEvent::MenuAction { .. } => "writ://menu-action",
        WritFrontendEvent::UpdateStatus(..) => "writ://update-status",
    };
    app.emit(event_name, &event).map_err(|e| e.to_string())
}
