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

use std::io::Read;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State};

use tracing::{info, warn};
use writ_core::buffer::BufferStatus;
use writ_core::hash::Sha256Digest;
use writ_core::notes::materialise::{DownloadOutcome, DOWNLOAD_TIMEOUT};
use writ_core::preview::protocol::SNIFF_BYTES;
use writ_core::preview::{
    check_inline_image_head, check_inline_image_stat, inline_image_route, render_contained,
    resolve_inline_image, AssetReference, AssetScope, ContentTypeId, InlineImageRefusal,
    RenderError, RenderRequest, RendererCapabilities, ThemePolarity,
};

use writ_storage::buffer_store::BufferStore;
use writ_storage::errors::StorageResult;
use writ_storage::layout_state::LayoutStateRecord;

use crate::commands::materialise::{materialise_with, read_and_verify_with};
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
    // The buffer's recorded scope keeps its token across re-renders and the
    // editor's image requests; core decides when a token may be kept.
    Some(state.preview_render_cache.scope_for(
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
/// file being missing or not downloaded is ordinary, and a toast for each one
/// while scrolling would be noise. The `Err` arm is kept for the cases that
/// mean the request itself could not be answered.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InlineImageResult {
    /// The image, served read-only from the note-asset route (ADR-035).
    Ready {
        /// The route URL in the form this platform's webview loads.
        url: String,
        /// MIME type decided by the leading bytes.
        mime: String,
        /// Length of the file.
        byte_len: u64,
    },
    /// The image is not drawn, and why.
    Refused {
        /// One of `outside_root`, `not_found`, `not_downloaded`, `too_large`,
        /// `not_an_image`.
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

/// Answer one image reference written in an open buffer with the URL the
/// editor draws it from.
///
/// No bytes cross IPC: the webview loads the image from the preview scheme,
/// which re-resolves the URL against the same scope. A file a sync provider
/// has not downloaded is refused from its stat and never read here.
#[tauri::command]
pub async fn preview_inline_image<R: Runtime>(
    app: AppHandle<R>,
    buffer_id: String,
    reference: String,
) -> Result<InlineImageResult, String> {
    run_blocking(app, ERR_COMMAND_STOPPED, move |state| {
        preview_inline_image_inner(
            state,
            &buffer_id,
            &reference,
            &writ_storage::notes_index::is_dataless,
        )
    })
    .await
}

/// Download one image a sync provider holds, then answer as
/// [`preview_inline_image`] does.
///
/// Reading the file is what asks the provider for it, so the read runs on a
/// thread of its own while a pool thread waits on it, bounded by
/// [`DOWNLOAD_TIMEOUT`]. A download that ends without the bytes answers
/// `not_downloaded` again.
#[tauri::command]
pub async fn preview_inline_image_download<R: Runtime>(
    app: AppHandle<R>,
    buffer_id: String,
    reference: String,
) -> Result<InlineImageResult, String> {
    run_blocking(app, ERR_COMMAND_STOPPED, move |state| {
        preview_inline_image_download_inner(
            state,
            &buffer_id,
            &reference,
            &writ_storage::notes_index::is_dataless,
            &download_from_provider,
        )
    })
    .await
}

/// [`preview_inline_image`] without Tauri, with the eviction probe injected.
///
/// The roots a reference resolves under are read off the buffer's row, as
/// [`asset_scope`] reads them for a render: the folder holding its file, or
/// the notes folder for a buffer with none. An id no open tab holds is
/// refused, so a root is never a folder the webview chose.
pub fn preview_inline_image_inner(
    state: &AppState,
    buffer_id: &str,
    reference: &str,
    is_dataless: &dyn Fn(&std::path::Path) -> bool,
) -> Result<InlineImageResult, String> {
    let (scope, found) = match inline_image_target(state, buffer_id, reference)? {
        Ok(target) => target,
        Err(refusal) => return Ok(InlineImageResult::refused(refusal)),
    };
    Ok(inline_image_answer(&scope, &found, is_dataless))
}

/// [`preview_inline_image_download`] without Tauri: `download` stands in for
/// the provider.
pub fn preview_inline_image_download_inner(
    state: &AppState,
    buffer_id: &str,
    reference: &str,
    is_dataless: &dyn Fn(&std::path::Path) -> bool,
    download: &dyn Fn(&std::path::Path) -> DownloadOutcome,
) -> Result<InlineImageResult, String> {
    let (scope, found) = match inline_image_target(state, buffer_id, reference)? {
        Ok(target) => target,
        Err(refusal) => return Ok(InlineImageResult::refused(refusal)),
    };
    if is_dataless(&found.path) {
        let outcome = download(&found.path);
        if outcome != DownloadOutcome::Done {
            warn!(
                buffer_id,
                outcome = ?outcome,
                "inline image download ended without the file"
            );
            return Ok(InlineImageResult::refused(
                InlineImageRefusal::NotDownloaded,
            ));
        }
    }
    Ok(inline_image_answer(&scope, &found, is_dataless))
}

/// Read `path` through its provider and wait for the outcome. The cancel flag
/// is never raised: the wait is ended by the bytes landing or the timeout.
fn download_from_provider(path: &std::path::Path) -> DownloadOutcome {
    let mut outcome = None;
    materialise_with(
        path.to_path_buf(),
        Arc::new(AtomicBool::new(false)),
        DOWNLOAD_TIMEOUT,
        |file| read_and_verify_with(file, writ_storage::notes_index::is_dataless),
        |ended| outcome = Some(ended),
    );
    outcome.unwrap_or_else(|| DownloadOutcome::Failed("the download did not report".to_string()))
}

/// The scope `buffer_id`'s images resolve under and the file `reference`
/// names inside it, or the refusal. The scope is recorded for the route,
/// keeping the token the buffer already has.
fn inline_image_target(
    state: &AppState,
    buffer_id: &str,
    reference: &str,
) -> Result<Result<(AssetScope, AssetReference), InlineImageRefusal>, String> {
    let Some(note_dir) = open_buffer_folder(state, buffer_id)? else {
        return Ok(Err(InlineImageRefusal::OutsideRoot));
    };
    let scope = state
        .preview_render_cache
        .scope_for(buffer_id, state.notes_root(), note_dir);
    Ok(
        resolve_inline_image(&scope.notes_root, &scope.note_dir, reference)
            .map(|found| (scope, found)),
    )
}

/// Decide a resolved file from its stat and its leading bytes, and mint its
/// URL. A placeholder is refused before anything opens it.
fn inline_image_answer(
    scope: &AssetScope,
    found: &AssetReference,
    is_dataless: &dyn Fn(&std::path::Path) -> bool,
) -> InlineImageResult {
    let Ok(metadata) = std::fs::metadata(&found.path) else {
        return InlineImageResult::refused(InlineImageRefusal::NotFound);
    };
    if !metadata.is_file() {
        return InlineImageResult::refused(InlineImageRefusal::NotFound);
    }
    if let Err(refusal) = check_inline_image_stat(metadata.len(), is_dataless(&found.path)) {
        return InlineImageResult::refused(refusal);
    }
    let head = match read_head(&found.path) {
        Ok(head) => head,
        Err(_) => return InlineImageResult::refused(InlineImageRefusal::NotFound),
    };
    let mime = match check_inline_image_head(&head) {
        Ok(mime) => mime,
        Err(refusal) => return InlineImageResult::refused(refusal),
    };
    let Some(route) = inline_image_route(scope, found) else {
        return InlineImageResult::refused(InlineImageRefusal::OutsideRoot);
    };
    InlineImageResult::Ready {
        url: format!("{}document/{route}", preview_url_prefix()),
        mime: mime.to_string(),
        byte_len: metadata.len(),
    }
}

/// The first [`SNIFF_BYTES`] of a file, enough to decide its type.
fn read_head(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    let mut head = Vec::with_capacity(SNIFF_BYTES);
    std::fs::File::open(path)?
        .take(SNIFF_BYTES as u64)
        .read_to_end(&mut head)?;
    Ok(head)
}

/// The folder an open buffer's images resolve against: the one holding its
/// file, or the notes folder for a buffer with none. `None` when no open tab
/// is that buffer. The store is let go before any file is touched.
fn open_buffer_folder(
    state: &AppState,
    buffer_id: &str,
) -> Result<Option<std::path::PathBuf>, String> {
    let row = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        store.find(buffer_id).map_err(|e| e.to_string())?
    };
    let Some(row) = row.filter(|row| row.status == BufferStatus::Active) else {
        return Ok(None);
    };
    Ok(match row.source_path {
        Some(source) => std::path::Path::new(&source)
            .parent()
            .map(std::path::Path::to_path_buf),
        None => Some(state.notes_root()),
    })
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
