//! Preview IPC commands — ADR-009 §"Consequences > src-tauri".
//!
//! - [`preview_list_renderers`] — registered content-type set for the
//!   frontend's `rendererRegistry` global.
//! - [`preview_open`] / [`preview_close`] — webview-slot assignment for a
//!   `(window, buffer)` pair.
//! - [`preview_render`] / [`preview_force_render`] — run the renderer over
//!   the live buffer text, cache the HTML for the protocol handler, and
//!   emit `preview:rendered` / `preview:error`. The frontend debounces
//!   keystroke-driven renders; `force_render` is the Cmd+R path (identical
//!   at the IPC boundary — the debounce gate is frontend-side).
//!
//! Phase 2b adds `preview_set_layout`, `preview_detach`, `preview_print`,
//! `preview_export` alongside the layout surface.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use writ_core::preview::{
    ContentTypeId, PreviewPolicy, RenderError, RenderRequest, RendererCapabilities, WindowId,
};

use crate::events::{emit_event, WritFrontendEvent};
use crate::preview::handler::RenderedDoc;
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

/// Release the webview slot assigned to `(window, buffer)` and drop its
/// cached render.
#[tauri::command]
pub fn preview_close(
    state: State<'_, AppState>,
    window_id: u64,
    buffer_id: String,
) {
    state
        .preview_webviews
        .close(WindowId(window_id), &buffer_id);
    state.preview_render_cache.evict(&buffer_id);
}

/// Outcome of a render request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreviewRenderResult {
    /// Rendered and cached; the webview should reload to pick it up.
    Rendered {
        /// Whether the host fallback stylesheet was injected.
        used_fallback_stylesheet: bool,
        /// Cheap parser warnings surfaced in the status chip.
        parser_warnings: Vec<String>,
    },
    /// No renderer registered for the content type — caller falls back to
    /// `Source`.
    NoRenderer {
        /// The content type that had no renderer.
        content_type: String,
    },
    /// The renderer refused or failed.
    Failed {
        /// Human-readable cause for the inline error card.
        message: String,
    },
}

fn run_render(
    app: &AppHandle,
    state: &AppState,
    window_id: u64,
    buffer_id: String,
    content_type: String,
    text: String,
) -> PreviewRenderResult {
    let ctype = ContentTypeId::new(content_type.clone());

    let registry = state
        .preview_registry
        .read()
        .expect("preview registry rwlock poisoned");
    let Some(renderer) = registry.get(&ctype) else {
        return PreviewRenderResult::NoRenderer { content_type };
    };

    // Phase 2 renders under the default SAFE policy; per-buffer session
    // policies arrive with the Phase 3 trust model.
    let policy = PreviewPolicy::Safe;
    let result = renderer.render(RenderRequest {
        content_type: ctype,
        buffer_text: text,
        workspace_root: None,
        policy,
    });
    drop(registry);

    match result {
        Ok(output) => {
            state.preview_render_cache.put(
                buffer_id.clone(),
                RenderedDoc {
                    html: output.document_html,
                    policy,
                },
            );
            let _ = emit_event(
                app,
                WritFrontendEvent::PreviewRendered {
                    buffer_id,
                    window_id,
                    used_fallback_stylesheet: output.used_fallback_stylesheet,
                    parser_warnings: output.parser_warnings.clone(),
                },
            );
            PreviewRenderResult::Rendered {
                used_fallback_stylesheet: output.used_fallback_stylesheet,
                parser_warnings: output.parser_warnings,
            }
        }
        Err(err) => {
            let message = render_error_message(&err);
            let _ = emit_event(
                app,
                WritFrontendEvent::PreviewError {
                    buffer_id,
                    window_id,
                    message: message.clone(),
                },
            );
            PreviewRenderResult::Failed { message }
        }
    }
}

fn render_error_message(err: &RenderError) -> String {
    match err {
        RenderError::DocumentTooLarge { bytes, limit } => {
            format!("document is {bytes} bytes; limit is {limit}")
        }
        RenderError::InvalidInput { reason } => reason.clone(),
        RenderError::Internal { reason } => reason.clone(),
    }
}

/// Render the live buffer text and cache the result for the protocol
/// handler. Invoked by the frontend debouncer after a keystroke.
#[tauri::command]
pub fn preview_render(
    app: AppHandle,
    state: State<'_, AppState>,
    window_id: u64,
    buffer_id: String,
    content_type: String,
    text: String,
) -> PreviewRenderResult {
    run_render(&app, &state, window_id, buffer_id, content_type, text)
}

/// Force a render regardless of frontend debounce gating (Cmd+R). Identical
/// to [`preview_render`] at the IPC boundary; the debounce lives frontend-
/// side.
#[tauri::command]
pub fn preview_force_render(
    app: AppHandle,
    state: State<'_, AppState>,
    window_id: u64,
    buffer_id: String,
    content_type: String,
    text: String,
) -> PreviewRenderResult {
    run_render(&app, &state, window_id, buffer_id, content_type, text)
}
