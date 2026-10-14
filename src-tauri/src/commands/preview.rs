//! Preview IPC commands — ADR-009 §"Consequences > src-tauri".
//!
//! - [`preview_list_renderers`] — registered content-type set for the
//!   frontend's `rendererRegistry` global.
//! - [`preview_open`] / [`preview_close`] — webview-slot assignment for a
//!   `(window, buffer)` pair.
//! - [`preview_render`] / [`preview_force_render`] — run the renderer over
//!   the live buffer text and cache the HTML for the protocol handler. The
//!   frontend debounces keystroke-driven renders; `force_render` is the Cmd+R
//!   path (identical at the IPC boundary — the debounce gate is frontend-side).
//!
//! Phase 2b adds `preview_set_layout`, `preview_detach`, `preview_print`,
//! `preview_export` alongside the layout surface.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State};

use tracing::{info, warn};
use writ_core::hash::Sha256Digest;
use writ_core::preview::{
    inline_image_data_url, render_contained, resolve_inline_image, size_within_inline_limit,
    AssetScope, ContentTypeId, InlineImageRefusal, RenderError, RenderRequest,
    RendererCapabilities, ThemePolarity,
};

use writ_storage::buffer_store::BufferStore;
use writ_storage::errors::StorageResult;
use writ_storage::layout_state::LayoutStateRecord;

use crate::commands::offload::{run_blocking, ERR_COMMAND_STOPPED};
use crate::poison::recover_poison;
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
    recover_poison(
        state.preview_registry.read(),
        "commands::preview::preview_list_renderers",
    )
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
/// document until the user opts the file in (decision RC-D1/TS-D1), and off
/// again once Writ holds bytes of the file the opt-in does not cover
/// (ADR-043).
#[tauri::command]
pub fn preview_get_scripts(state: State<'_, AppState>, buffer_id: String) -> Result<bool, String> {
    preview_get_scripts_inner(&state, &buffer_id)
}

/// [`preview_get_scripts`] without Tauri: what the next render of the file
/// decides, from its row and the digest of the bytes Writ last read from the
/// file or wrote to it. No file is read, so the status bar never waits on one
/// that does not answer. An id the store does not hold answers off.
pub fn preview_get_scripts_inner(state: &AppState, buffer_id: &str) -> Result<bool, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    scripts_may_run(state, &store, buffer_id).map_err(|e| e.to_string())
}

/// Record whether this file's own scripts may run, remembered on its buffer
/// row with the digest of the bytes the opt-in covers. The frontend
/// force-refreshes after this so the iframe reloads under the new document
/// CSP.
#[tauri::command]
pub fn preview_set_scripts(
    state: State<'_, AppState>,
    buffer_id: String,
    enabled: bool,
) -> Result<(), String> {
    preview_set_scripts_inner(&state, &buffer_id, enabled)
}

/// [`preview_set_scripts`] without Tauri. An opt-in covers the bytes Writ
/// holds for the file, the ones the document on screen was built from, so no
/// file is read. An id the store does not hold is refused, so the caller
/// never reports a choice that was not recorded.
pub fn preview_set_scripts_inner(
    state: &AppState,
    buffer_id: &str,
    enabled: bool,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if enabled {
        return store
            .enable_preview_scripts(buffer_id, bytes_writ_holds(state, buffer_id))
            .map_err(|e| e.to_string());
    }
    store
        .disable_preview_scripts(buffer_id)
        .map_err(|e| e.to_string())?;
    state.preview_render_cache.turn_scripts_off(buffer_id);
    Ok(())
}

/// The digest of the bytes Writ last read from `buffer_id`'s file or wrote to
/// it.
///
/// Asked with the store held: the save and the editor's read move this record
/// while they hold the store, so the record and the row read together describe
/// the same moment.
fn bytes_writ_holds(state: &AppState, buffer_id: &str) -> Option<Sha256Digest> {
    state.disk_state(buffer_id).map(|disk| disk.hash)
}

/// Whether `buffer_id`'s own scripts may run now, checked against the bytes
/// Writ holds for its file, with `store` the guard the caller holds. When they
/// may not, the document already rendered from the file is served with them
/// off from now on.
///
/// The render cache's lock is taken under the store's. Nothing holds that lock
/// while it takes another, so the order cannot turn round.
fn scripts_may_run(state: &AppState, store: &BufferStore, buffer_id: &str) -> StorageResult<bool> {
    let verdict = store.check_preview_scripts(buffer_id, bytes_writ_holds(state, buffer_id));
    if !matches!(verdict, Ok(true)) {
        state.preview_render_cache.turn_scripts_off(buffer_id);
    }
    verdict
}

/// Turn off the scripts of `buffer_id`'s file, in its row and in the document
/// already rendered from it, because a program other than the editor changed
/// the file (ADR-043). Answers whether they were on.
///
/// `store` is the guard the caller already holds: the emitters that report the
/// change hold it while they do, and the bus runs its subscribers on the
/// emitting thread, so the lapse is made here rather than in a subscriber. No
/// file is read, and the render cache's lock is a leaf. A row that cannot be
/// read or written is logged; the next check compares the bytes Writ holds
/// with the recorded digest and turns the scripts off then.
pub fn lapse_scripts_opt_in(store: &BufferStore, cache: &RenderCache, buffer_id: &str) -> bool {
    cache.turn_scripts_off(buffer_id);
    match store.preview_scripts(buffer_id) {
        Ok(true) => match store.disable_preview_scripts(buffer_id) {
            Ok(()) => {
                info!(
                    buffer_id,
                    "scripts turned off: the file changed outside the editor"
                );
                true
            }
            Err(e) => {
                warn!(buffer_id, error = %e, "could not turn off the scripts of a changed file");
                false
            }
        },
        Ok(false) => false,
        Err(e) => {
            warn!(buffer_id, error = %e, "could not read the scripts opt-in of a changed file");
            false
        }
    }
}

/// [`lapse_scripts_opt_in`] for a caller that does not hold the store: it is
/// taken for the row's read and write and let go. A caller holding it would wait on
/// itself, so one that does calls [`lapse_scripts_opt_in`] with its guard.
pub fn lapse_scripts_opt_in_now(state: &AppState, buffer_id: &str) -> bool {
    let store = recover_poison(
        state.store.lock(),
        "commands::preview::lapse_scripts_opt_in_now",
    );
    lapse_scripts_opt_in(&store, &state.preview_render_cache, buffer_id)
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

/// Persist a buffer's preview layout.
///
/// `path` is the buffer's absolute source path, or `None` for a scratch
/// buffer. Scratch buffers are not persisted (ADR-009): they always reopen
/// in the content-type default.
#[tauri::command]
pub fn preview_set_layout(
    state: State<'_, AppState>,
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
        /// Whether the document is served with its own scripts on: the opt-in
        /// as this render checked it, which the status bar shows.
        scripts_enabled: bool,
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
        /// Whether the document is served with its own scripts on.
        scripts_enabled: bool,
    },
    /// No renderer is registered for the request's content type.
    NoRenderer,
    /// The renderer refused or failed.
    Failed(RenderError),
}

/// [`preview_render`] without Tauri: render the request with the renderer
/// registered for its content type and cache the document for the protocol
/// handler, with the request's asset scope and the file's scripts opt-in. The
/// handler serves the cached document under the CSP that opt-in calls for.
///
/// The renderer runs without the store. The opt-in is checked and the document
/// cached under one hold of the store, which every lapse of an opt-in takes
/// too, so a lapse lands either before the check or after the document is in
/// the cache, where it turns the scripts off. The check reads no file
/// ([`preview_get_scripts_inner`]). An id the store does not hold, a store that
/// cannot be read, or bytes the opt-in does not cover render with scripts off.
pub fn render_into_cache(
    state: &AppState,
    buffer_id: &str,
    request: RenderRequest,
) -> CachedRender {
    let registry = recover_poison(
        state.preview_registry.read(),
        "commands::preview::render_into_cache",
    );
    let Some(renderer) = registry.get(&request.content_type) else {
        return CachedRender::NoRenderer;
    };
    let assets = request.assets.clone();
    let result = render_contained(|| renderer.render(request));
    drop(registry);

    match result {
        Ok(output) => {
            let doc = |scripts_enabled| RenderedDoc {
                html: output.document_html,
                assets,
                scripts_enabled,
            };
            let scripts_enabled = match state.store.lock() {
                Ok(store) => {
                    let scripts_enabled =
                        scripts_may_run(state, &store, buffer_id).unwrap_or(false);
                    state
                        .preview_render_cache
                        .put(buffer_id, doc(scripts_enabled));
                    scripts_enabled
                }
                Err(_) => {
                    state.preview_render_cache.put(buffer_id, doc(false));
                    false
                }
            };
            CachedRender::Rendered {
                used_fallback_stylesheet: output.used_fallback_stylesheet,
                parser_warnings: output.parser_warnings,
                scripts_enabled,
            }
        }
        Err(err) => CachedRender::Failed(err),
    }
}

fn run_render(
    state: &AppState,
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
    match render_into_cache(state, &buffer_id, request) {
        CachedRender::NoRenderer => PreviewRenderResult::NoRenderer { content_type },
        CachedRender::Rendered {
            used_fallback_stylesheet,
            parser_warnings,
            scripts_enabled,
        } => PreviewRenderResult::Rendered {
            used_fallback_stylesheet,
            parser_warnings,
            scripts_enabled,
        },
        CachedRender::Failed(err) => PreviewRenderResult::Failed {
            message: render_error_message(&err),
        },
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
///
/// Off the main thread, and in the buffer's render queue: renders of one
/// buffer run one at a time in the order they were asked for, so the cache
/// ends on the newest text (ADR-047).
#[tauri::command]
pub async fn preview_render<R: Runtime>(
    app: AppHandle<R>,
    buffer_id: String,
    content_type: String,
    text: String,
    theme: ThemePolarity,
    zoom: f64,
) -> Result<PreviewRenderResult, String> {
    render_in_order(app, buffer_id, content_type, text, theme, zoom).await
}

/// Force a render regardless of frontend debounce gating (Cmd+R). Identical
/// to [`preview_render`] at the IPC boundary; the debounce lives frontend-
/// side.
#[tauri::command]
pub async fn preview_force_render<R: Runtime>(
    app: AppHandle<R>,
    buffer_id: String,
    content_type: String,
    text: String,
    theme: ThemePolarity,
    zoom: f64,
) -> Result<PreviewRenderResult, String> {
    render_in_order(app, buffer_id, content_type, text, theme, zoom).await
}

/// [`run_render`] on the blocking pool, after every render of the buffer that
/// was asked for before it.
async fn render_in_order<R: Runtime>(
    app: AppHandle<R>,
    buffer_id: String,
    content_type: String,
    text: String,
    theme: ThemePolarity,
    zoom: f64,
) -> Result<PreviewRenderResult, String> {
    let ticket = app
        .state::<AppState>()
        .preview_render_cache
        .render_ticket(&buffer_id);
    run_blocking(app, ERR_COMMAND_STOPPED, move |state| {
        let _turn = ticket.wait();
        Ok(run_render(
            state,
            buffer_id,
            content_type,
            text,
            theme,
            zoom,
        ))
    })
    .await
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
pub async fn preview_inline_image<R: Runtime>(
    app: AppHandle<R>,
    note_path: Option<String>,
    reference: String,
) -> Result<InlineImageResult, String> {
    run_blocking(app, ERR_COMMAND_STOPPED, move |state| {
        preview_inline_image_inner(state, note_path.as_deref(), &reference)
    })
    .await
}

/// [`preview_inline_image`] without Tauri.
///
/// The folder a reference resolves against is the one holding the file of an
/// open tab, read off that tab's row as [`asset_scope`] does. The path the
/// webview sends only picks the tab: one no open tab holds is refused, so the
/// containment root is never a folder the webview chose.
pub fn preview_inline_image_inner(
    state: &AppState,
    note_path: Option<&str>,
    reference: &str,
) -> Result<InlineImageResult, String> {
    let notes_root = state.notes_root();
    let note_dir = match note_path {
        None => notes_root.clone(),
        Some(path) => match open_file_folder(state, path)? {
            Some(dir) => dir,
            None => return Ok(InlineImageResult::refused(InlineImageRefusal::OutsideRoot)),
        },
    };

    let path = match resolve_inline_image(&notes_root, &note_dir, reference) {
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

/// The folder holding the file an open tab shows, by that file's path, or
/// `None` when no open tab holds it. The store is let go before any file is
/// touched.
fn open_file_folder(state: &AppState, path: &str) -> Result<Option<std::path::PathBuf>, String> {
    let source_path = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        store
            .find_active_by_source_path(path)
            .map_err(|e| e.to_string())?
            .and_then(|doc| doc.source_path)
    };
    Ok(source_path.and_then(|source| {
        std::path::Path::new(&source)
            .parent()
            .map(std::path::Path::to_path_buf)
    }))
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
