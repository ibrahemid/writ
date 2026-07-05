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
    ContentTypeId, RenderError, RenderRequest, RendererCapabilities, ThemePolarity,
};

use writ_storage::layout_state::LayoutStateRecord;

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

/// Drop the cached render for a buffer when its preview pane closes.
///
/// Under the iframe substrate there is no Rust-side webview to tear down —
/// the iframe is a DOM element the frontend unmounts. The only host-side
/// resource is the render cache entry, which this releases.
#[tauri::command]
pub fn preview_close(state: State<'_, AppState>, buffer_id: String) {
    state.preview_render_cache.evict(&buffer_id);
}

/// A buffer's persisted layout, returned to the frontend on open.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedLayout {
    /// Layout discriminant: `source` | `split` | `preview`.
    pub layout: String,
    /// Split ratio, present only for `split`.
    pub ratio: Option<f32>,
}

/// Fetch the persisted layout for a source-backed buffer, if any. Scratch
/// buffers (no path) never persist and always resolve to the content-type
/// default on the frontend.
#[tauri::command]
pub fn preview_get_layout(
    state: State<'_, AppState>,
    path: String,
) -> Result<Option<PersistedLayout>, String> {
    let record = state.layout_state.get(&path).map_err(|e| e.to_string())?;
    Ok(record.map(|r| PersistedLayout {
        layout: r.layout_mode,
        ratio: r.split_ratio,
    }))
}

/// Persist a buffer's preview layout and broadcast the change.
///
/// `path` is the buffer's absolute source path, or `None` for a scratch
/// buffer. Scratch buffers are not persisted (ADR-009) — they always reopen
/// in the content-type default — but the `LayoutChanged` event still fires
/// so other views in the window stay in sync.
#[tauri::command]
pub fn preview_set_layout(
    app: AppHandle,
    state: State<'_, AppState>,
    window_id: u64,
    buffer_id: String,
    path: Option<String>,
    layout: String,
    ratio: Option<f32>,
) -> Result<(), String> {
    if let Some(path) = &path {
        let last_view_mode = if layout == "source" {
            "source"
        } else {
            "preview"
        };
        state
            .layout_state
            .set(&LayoutStateRecord {
                path: path.clone(),
                layout_mode: layout.clone(),
                split_ratio: ratio,
                last_view_mode: last_view_mode.to_string(),
            })
            .map_err(|e| e.to_string())?;
    }

    let _ = emit_event(
        &app,
        WritFrontendEvent::LayoutChanged {
            buffer_id,
            window_id,
            layout,
            ratio,
        },
    );
    Ok(())
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

#[allow(clippy::too_many_arguments)]
fn run_render(
    app: &AppHandle,
    state: &AppState,
    window_id: u64,
    buffer_id: String,
    content_type: String,
    text: String,
    theme: ThemePolarity,
    zoom: f64,
) -> PreviewRenderResult {
    let ctype = ContentTypeId::new(content_type.clone());

    let registry = state
        .preview_registry
        .read()
        .expect("preview registry rwlock poisoned");
    let Some(renderer) = registry.get(&ctype) else {
        return PreviewRenderResult::NoRenderer { content_type };
    };

    // Lean scope: one fixed CSP, no per-document policy. The scripts kill
    // switch is applied at serve time, not here.
    let result = renderer.render(RenderRequest {
        content_type: ctype,
        buffer_text: text,
        theme,
        zoom,
    });
    drop(registry);

    match result {
        Ok(output) => {
            state.preview_render_cache.put(
                buffer_id.clone(),
                RenderedDoc {
                    html: output.document_html,
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
#[allow(clippy::too_many_arguments)]
pub fn preview_render(
    app: AppHandle,
    state: State<'_, AppState>,
    window_id: u64,
    buffer_id: String,
    content_type: String,
    text: String,
    theme: ThemePolarity,
    zoom: f64,
) -> PreviewRenderResult {
    run_render(
        &app,
        &state,
        window_id,
        buffer_id,
        content_type,
        text,
        theme,
        zoom,
    )
}

/// Force a render regardless of frontend debounce gating (Cmd+R). Identical
/// to [`preview_render`] at the IPC boundary; the debounce lives frontend-
/// side.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn preview_force_render(
    app: AppHandle,
    state: State<'_, AppState>,
    window_id: u64,
    buffer_id: String,
    content_type: String,
    text: String,
    theme: ThemePolarity,
    zoom: f64,
) -> PreviewRenderResult {
    run_render(
        &app,
        &state,
        window_id,
        buffer_id,
        content_type,
        text,
        theme,
        zoom,
    )
}
