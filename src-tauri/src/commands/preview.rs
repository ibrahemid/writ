//! Phase 1 preview IPC commands — ADR-009 §"Consequences > src-tauri".
//!
//! The exposed surface is intentionally small for Phase 1:
//!
//! - [`preview_list_renderers`] — returns the registered content-type set
//!   so the frontend's `rendererRegistry` global can populate at boot.
//! - [`preview_open`] — assigns a webview slot to a `(window, buffer)`
//!   pair. Phase 1 does not yet spawn the actual `WebviewWindow`; the
//!   slot bookkeeping is in place so Phase 2's `<PreviewPane>` mount
//!   path lights up without a manager rewrite.
//! - [`preview_close`] — releases the slot for a `(window, buffer)`.
//!
//! Phase 2 adds `preview_set_layout`, `preview_render`, `preview_force_render`,
//! `preview_detach`, `preview_print`, `preview_export` per the ADR.

use serde::{Deserialize, Serialize};
use tauri::State;

use writ_core::preview::{ContentTypeId, RendererCapabilities, WindowId};

use crate::state::AppState;

/// Serialized renderer descriptor surfaced to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RendererInfo {
    /// Content-type identifier (the registry key).
    pub content_type: String,
    /// Capabilities advertised by the renderer.
    pub capabilities: RendererCapabilities,
}

/// List every registered renderer.
#[tauri::command]
pub fn preview_list_renderers(state: State<'_, AppState>) -> Vec<RendererInfo> {
    state
        .preview_registry
        .read()
        .expect("preview registry rwlock poisoned")
        .list()
        .into_iter()
        .map(|(id, capabilities)| RendererInfo {
            content_type: id.as_str().to_string(),
            capabilities,
        })
        .collect()
}

/// Outcome of [`preview_open`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewOpenOutcome {
    /// Webview slot id assigned to the (window, buffer) pair.
    pub slot_id: u64,
    /// `true` if the slot came from the warm pool (warm spawn path),
    /// `false` if it was created cold.
    pub warm: bool,
}

/// Outcome variants returned to the frontend as discriminated unions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreviewOpenResult {
    /// Slot assigned successfully.
    Assigned(PreviewOpenOutcome),
    /// Requested content type has no registered renderer; the caller
    /// should fall back to `Source` layout.
    NoRendererForType {
        /// Content type id that was requested.
        content_type: String,
    },
}

/// Assign a webview slot to a `(window, buffer)` pair.
#[tauri::command]
pub fn preview_open(
    state: State<'_, AppState>,
    window_id: u64,
    buffer_id: String,
    content_type: String,
) -> PreviewOpenResult {
    let ctype = ContentTypeId::new(content_type.clone());

    let registry = state
        .preview_registry
        .read()
        .expect("preview registry rwlock poisoned");
    if !registry.has(&ctype) {
        return PreviewOpenResult::NoRendererForType { content_type };
    }
    drop(registry);

    let window_id = WindowId(window_id);
    let manager = state.preview_webviews.clone();

    match manager.take_warm_for(window_id, &buffer_id, ctype) {
        Some(slot) => PreviewOpenResult::Assigned(PreviewOpenOutcome {
            slot_id: slot.0,
            warm: true,
        }),
        None => {
            // Phase 1 cold-spawn placeholder: record a fresh slot
            // immediately so the assignment table reflects intent. Phase
            // 2's real cold-spawn path constructs an actual webview here
            // and reports the measured time against the cold-spawn
            // budget.
            let slot = manager.record_warm_slot();
            let assigned = manager
                .take_warm_for(window_id, &buffer_id, ContentTypeId::new(content_type))
                .expect("just-recorded slot should be takeable");
            // The slot id we created may differ from the one assigned
            // (since the assignment path returns whichever slot is at
            // the top of the pool). We surface the assigned one.
            let _ = slot;
            PreviewOpenResult::Assigned(PreviewOpenOutcome {
                slot_id: assigned.0,
                warm: false,
            })
        }
    }
}

/// Release the webview slot assigned to `(window, buffer)`.
#[tauri::command]
pub fn preview_close(
    state: State<'_, AppState>,
    window_id: u64,
    buffer_id: String,
) {
    state
        .preview_webviews
        .close(WindowId(window_id), &buffer_id);
}
