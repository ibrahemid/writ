use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
#[serde(tag = "kind", content = "payload")]
pub enum WritFrontendEvent {
    #[serde(rename = "config:changed")]
    ConfigChanged { keys: Vec<String> },

    #[serde(rename = "buffer:external")]
    BufferExternal { buffer_id: String, change: String },

    #[serde(rename = "recovery:dirty")]
    RecoveryDirty {
        snapshot_id: String,
        buffer_count: u32,
    },
}

pub fn emit_event(app: &AppHandle, event: WritFrontendEvent) -> Result<(), String> {
    let event_name = match &event {
        WritFrontendEvent::ConfigChanged { .. } => "writ://config-changed",
        WritFrontendEvent::BufferExternal { .. } => "writ://buffer-external",
        WritFrontendEvent::RecoveryDirty { .. } => "writ://recovery-dirty",
    };
    app.emit(event_name, &event).map_err(|e| e.to_string())
}
