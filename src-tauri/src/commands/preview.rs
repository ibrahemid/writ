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

use std::sync::{Mutex, RwLock};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use writ_core::preview::{
    inline_image_data_url, resolve_inline_image, size_within_inline_limit, AssetScope,
    ContentRendererRegistry, ContentTypeId, InlineImageRefusal, RenderError, RenderRequest,
    RendererCapabilities, ThemePolarity,
};

use writ_storage::buffer_store::BufferStore;
use writ_storage::layout_state::LayoutStateRecord;

use crate::events::{emit_event, WritFrontendEvent};
use crate::preview::handler::{RenderCache, RenderedDoc};
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

/// The base a preview URL is built from, decided by the host, not guessed by
/// the frontend. On Windows WebView2 serves a custom scheme as
/// `http://<scheme>.<host>`, so the preview iframe, its runtimes and its
/// images must request `http://writ-preview.…`; everywhere else the native
/// `writ-preview://` form works (TS-03/DS-03).
#[tauri::command]
pub fn preview_url_prefix() -> &'static str {
    preview_url_prefix_for(cfg!(target_os = "windows"))
}

/// The prefix for a given platform, split out so both forms are unit-tested
/// without a Windows host.
fn preview_url_prefix_for(is_windows: bool) -> &'static str {
    if is_windows {
        "http://writ-preview."
    } else {
        "writ-preview://"
    }
}

/// Whether this file's own scripts may run in its preview. Off for every
/// document until the user opts the file in (decision RC-D1/TS-D1).
#[tauri::command]
pub fn preview_get_scripts(state: State<'_, AppState>, buffer_id: String) -> Result<bool, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    preview_get_scripts_inner(&store, &buffer_id)
}

/// [`preview_get_scripts`] without Tauri: an id the store does not hold
/// answers off.
pub fn preview_get_scripts_inner(store: &BufferStore, buffer_id: &str) -> Result<bool, String> {
    store.preview_scripts(buffer_id).map_err(|e| e.to_string())
}

/// Record whether this file's own scripts may run, remembered on its buffer
/// row. The frontend force-refreshes after this so the iframe reloads under
/// the new document CSP.
#[tauri::command]
pub fn preview_set_scripts(
    state: State<'_, AppState>,
    buffer_id: String,
    enabled: bool,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    preview_set_scripts_inner(&store, &buffer_id, enabled)
}

/// [`preview_set_scripts`] without Tauri: an id the store does not hold is
/// refused, so the caller never reports a choice that was not recorded.
pub fn preview_set_scripts_inner(
    store: &BufferStore,
    buffer_id: &str,
    enabled: bool,
) -> Result<(), String> {
    store
        .set_preview_scripts(buffer_id, enabled)
        .map_err(|e| e.to_string())
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

/// Where a buffer's embedded files may be read from: the notes folder and
/// the folder holding the buffer's own file.
///
/// `None` for a buffer with no file on disk — a scratch note embeds nothing,
/// because there is no folder to resolve a relative reference against.
/// ADR-035.
fn asset_scope(state: &AppState, buffer_id: &str) -> Option<AssetScope> {
    let source_path = {
        let store = state.store.lock().ok()?;
        store.get(buffer_id).ok()?.source_path?
    };
    // The previous render's scope is handed to core so its token survives a
    // re-render of the same note; core decides when a token may be kept.
    Some(AssetScope::for_render(
        state.preview_render_cache.scope(buffer_id).as_ref(),
        buffer_id,
        state.notes_root(),
        std::path::Path::new(&source_path).parent()?.to_path_buf(),
    ))
}

/// What [`render_into_cache`] did with a render request.
#[derive(Debug)]
pub enum CachedRender {
    /// The document is in the render cache.
    Rendered {
        /// Whether the host fallback stylesheet was injected.
        used_fallback_stylesheet: bool,
        /// Cheap parser warnings surfaced in the status chip.
        parser_warnings: Vec<String>,
    },
    /// No renderer is registered for the request's content type.
    NoRenderer,
    /// The renderer refused or failed.
    Failed(RenderError),
}

/// [`preview_render`] without Tauri: render the request with the renderer
/// registered for its content type and cache the document for the protocol
/// handler, with the request's asset scope and the file's scripts opt-in, read
/// from the store now. The handler serves the cached document under the CSP
/// that opt-in calls for. An id the store does not hold, or a store that
/// cannot be read, renders with scripts off.
pub fn render_into_cache(
    store: &Mutex<BufferStore>,
    registry: &RwLock<ContentRendererRegistry>,
    cache: &RenderCache,
    buffer_id: &str,
    request: RenderRequest,
) -> CachedRender {
    let registry = registry.read().expect("preview registry rwlock poisoned");
    let Some(renderer) = registry.get(&request.content_type) else {
        return CachedRender::NoRenderer;
    };
    let scripts_enabled = store
        .lock()
        .ok()
        .and_then(|store| store.preview_scripts(buffer_id).ok())
        .unwrap_or(false);
    let assets = request.assets.clone();
    let result = renderer.render(request);
    drop(registry);

    match result {
        Ok(output) => {
            cache.put(
                buffer_id,
                RenderedDoc {
                    html: output.document_html,
                    assets,
                    scripts_enabled,
                },
            );
            CachedRender::Rendered {
                used_fallback_stylesheet: output.used_fallback_stylesheet,
                parser_warnings: output.parser_warnings,
            }
        }
        Err(err) => CachedRender::Failed(err),
    }
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
    let request = RenderRequest {
        content_type: ContentTypeId::new(content_type.clone()),
        buffer_text: text,
        theme,
        zoom,
        assets: asset_scope(state, &buffer_id),
    };
    match render_into_cache(
        &state.store,
        &state.preview_registry,
        &state.preview_render_cache,
        &buffer_id,
        request,
    ) {
        CachedRender::NoRenderer => PreviewRenderResult::NoRenderer { content_type },
        CachedRender::Rendered {
            used_fallback_stylesheet,
            parser_warnings,
        } => {
            let _ = emit_event(
                app,
                WritFrontendEvent::PreviewRendered {
                    buffer_id,
                    window_id,
                    used_fallback_stylesheet,
                    parser_warnings: parser_warnings.clone(),
                },
            );
            PreviewRenderResult::Rendered {
                used_fallback_stylesheet,
                parser_warnings,
            }
        }
        CachedRender::Failed(err) => {
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

/// Outcome of a request for one inline image.
///
/// A refusal is an outcome the editor draws as a caption, not an error: the
/// file being missing or oversized is ordinary, and a toast for each one
/// while scrolling would be noise. The `Err` arm is kept for the cases that
/// mean the request itself could not be answered.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InlineImageResult {
    /// The bytes, encoded for an `img` element.
    Ready {
        /// `data:<mime>;base64,…`.
        data_url: String,
        /// MIME type decided by the leading bytes.
        mime: String,
        /// Length of the file the URL holds.
        byte_len: u64,
    },
    /// The image is not carried inline, and why.
    Refused {
        /// One of `outside_root`, `not_found`, `too_large`, `not_an_image`.
        reason: String,
    },
}

impl InlineImageResult {
    fn refused(refusal: InlineImageRefusal) -> Self {
        Self::Refused {
            reason: refusal.as_str().to_string(),
        }
    }
}

/// Read one image referenced by an open file and answer with a data URL.
///
/// `note_path` is the file the reference was written in; a buffer with no
/// file resolves against the notes folder alone. The size refusal is decided
/// on the metadata, so a file over the cap is never read.
#[tauri::command]
pub fn preview_inline_image(
    state: State<'_, AppState>,
    note_path: Option<String>,
    reference: String,
) -> Result<InlineImageResult, String> {
    let notes_root = state.notes_root();
    let note_dir = note_path
        .as_deref()
        .map(std::path::Path::new)
        .and_then(|path| path.parent())
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| notes_root.clone());

    let path = match resolve_inline_image(&notes_root, &note_dir, &reference) {
        Ok(path) => path,
        Err(refusal) => return Ok(InlineImageResult::refused(refusal)),
    };
    let metadata = std::fs::metadata(&path).map_err(|err| err.to_string())?;
    if !metadata.is_file() {
        return Ok(InlineImageResult::refused(InlineImageRefusal::NotFound));
    }
    if let Err(refusal) = size_within_inline_limit(metadata.len()) {
        return Ok(InlineImageResult::refused(refusal));
    }
    let bytes = std::fs::read(&path).map_err(|err| err.to_string())?;
    match inline_image_data_url(&bytes) {
        Ok(image) => Ok(InlineImageResult::Ready {
            data_url: image.data_url,
            mime: image.mime,
            byte_len: image.byte_len,
        }),
        Err(refusal) => Ok(InlineImageResult::refused(refusal)),
    }
}

#[cfg(test)]
mod tests {
    use super::{preview_url_prefix, preview_url_prefix_for};

    #[test]
    fn windows_preview_urls_use_the_http_workaround_form() {
        assert_eq!(preview_url_prefix_for(true), "http://writ-preview.");
    }

    #[test]
    fn other_platforms_use_the_native_scheme() {
        assert_eq!(preview_url_prefix_for(false), "writ-preview://");
    }

    #[test]
    fn this_build_hands_out_its_own_platforms_prefix() {
        assert_eq!(
            preview_url_prefix(),
            preview_url_prefix_for(cfg!(target_os = "windows"))
        );
    }
}
