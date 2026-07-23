import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize, LogicalPosition } from "@tauri-apps/api/window";
import type { BufferDocument, FileOpenResult } from "../types/buffer";
import type { WritConfig } from "../types/config";
import type { TransformDescriptor } from "../types/transforms";
import type { ThemePolarity } from "../types/theme";

export async function listTransforms(): Promise<TransformDescriptor[]> {
  return invoke("list_transforms");
}

export async function applyTransform(transformId: string, input: string): Promise<string> {
  return invoke("apply_transform", { transformId, input });
}

export async function promptEstimateTokens(text: string): Promise<number> {
  return invoke("prompt_estimate_tokens", { text });
}

export async function promptScanPlaceholders(text: string): Promise<string[]> {
  return invoke("prompt_scan_placeholders", { text });
}

export async function promptFillPlaceholders(
  text: string,
  values: Record<string, string>,
): Promise<string> {
  return invoke("prompt_fill_placeholders", { text, values });
}

import type { SpellingLint } from "../types/spelling";
export type { SpellingLint };

export async function checkSpelling(text: string): Promise<SpellingLint[]> {
  return invoke("check_spelling", { text });
}

export async function spellingAddIgnoredWord(word: string): Promise<void> {
  return invoke("spelling_add_ignored_word", { word });
}

export async function createBuffer(title?: string): Promise<BufferDocument> {
  return invoke("create_buffer", { title: title ?? null });
}

export async function getBuffer(id: string): Promise<BufferDocument> {
  return invoke("get_buffer", { id });
}

export async function saveBufferContent(id: string, content: string): Promise<void> {
  return invoke("save_buffer_content", { id, content });
}

export async function readBufferContent(id: string): Promise<string> {
  const bytes = await invoke<ArrayBuffer>("read_buffer_content", { id });
  return new TextDecoder().decode(bytes);
}

export async function listActiveBuffers(): Promise<BufferDocument[]> {
  return invoke("list_active_buffers");
}

export async function closeBuffer(id: string): Promise<void> {
  return invoke("close_buffer", { id });
}

export async function closeBuffers(ids: string[]): Promise<void> {
  return invoke("close_buffers", { ids });
}

export async function reportFirstPaint(
  elapsedMs: number,
  mode: "cold" | "warm",
  rustElapsedUs: number | null = null,
): Promise<void> {
  return invoke("report_first_paint", {
    elapsedMs,
    mode,
    rustElapsedUs,
  });
}

export async function deleteBuffer(id: string): Promise<void> {
  return invoke("delete_buffer", { id });
}

export async function updateTabOrder(id: string, order: number): Promise<void> {
  return invoke("update_tab_order", { id, order });
}

export async function listHistory(): Promise<BufferDocument[]> {
  return invoke("list_history");
}

export async function restoreBuffer(id: string): Promise<void> {
  return invoke("restore_buffer", { id });
}

export async function clearHistory(): Promise<void> {
  return invoke("clear_history");
}

import type { SearchResults } from "../types/search";
export type { SnippetSegment, SearchHit, SearchResults } from "../types/search";

export async function searchBuffers(query: string): Promise<SearchResults> {
  return invoke("search_buffers", { query });
}

export async function getConfig(): Promise<WritConfig> {
  return invoke("get_config");
}

export async function updateConfig(config: WritConfig): Promise<void> {
  return invoke("update_config", { config });
}

export async function toggleWindow(): Promise<void> {
  return invoke("toggle_window");
}

export async function openFile(path: string): Promise<FileOpenResult> {
  return invoke("open_file", { path });
}

export async function openFileConfirmed(path: string): Promise<FileOpenResult> {
  return invoke("open_file_confirmed", { path });
}

export async function saveToSource(id: string, content: string): Promise<void> {
  return invoke("save_to_source", { id, content });
}

export async function showOpenFileDialog(): Promise<string | null> {
  const paths = await invoke<string[]>("pick_files_to_open");
  if (Array.isArray(paths) && paths.length > 0) {
    return paths[0];
  }
  return null;
}

export async function renameBuffer(id: string, title: string): Promise<void> {
  return invoke("rename_buffer", { id, title });
}

export interface RecoveredBuffer {
  id: string;
  content: string;
}

export async function getRecoveredBuffers(): Promise<RecoveredBuffer[]> {
  return invoke("get_recovered_buffers");
}

export async function checkForUpdate(): Promise<void> {
  return invoke("check_for_update");
}

export async function downloadAndInstallUpdate(): Promise<void> {
  return invoke("download_and_install_update");
}

export async function dismissUpdate(): Promise<void> {
  return invoke("dismiss_update");
}

export async function restartApp(): Promise<void> {
  return invoke("restart_app");
}

const FULLSCREEN_EXIT_TIMEOUT_MS = 700;

// Leave native fullscreen and wait for the OS transition to finish before
// returning. setFullscreen() resolves when the IPC dispatches, not when the
// macOS exit animation completes, so a minimize()/hide() issued straight after
// would land mid-transition and be dropped by AppKit. Settle on the window's
// first resize after the toggle (the transition's end), with a timeout as a
// safety net for platforms that report no resize.
async function exitFullscreen(win: ReturnType<typeof getCurrentWindow>): Promise<void> {
  let settle: () => void = () => {};
  const transitioned = new Promise<void>((resolve) => {
    settle = resolve;
  });
  const unlisten = await win.onResized(() => settle());
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    await win.setFullscreen(false);
    await Promise.race([
      transitioned,
      new Promise<void>((resolve) => {
        timer = setTimeout(resolve, FULLSCREEN_EXIT_TIMEOUT_MS);
      }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
    unlisten();
  }
}

export async function hideWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
    if (await win.isFullscreen()) {
      await exitFullscreen(win);
    }
    await win.hide();
  } catch (err) {
    console.warn("hideWindow failed:", err);
  }
}

// The window is created hidden to avoid the cold-start flash; the frontend
// reveals it after its first paint (App onMount). Geometry is already restored
// in Rust setup, so this only shows and focuses.
export async function showWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
    await win.show();
    await win.setFocus();
  } catch (err) {
    console.warn("showWindow failed:", err);
  }
}

export async function minimizeWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
    if (await win.isFullscreen()) {
      await exitFullscreen(win);
    }
    await win.minimize();
  } catch (err) {
    console.warn("minimizeWindow failed:", err);
  }
}

export async function startDraggingWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
    await win.startDragging();
  } catch (err) {
    console.warn("startDraggingWindow failed:", err);
  }
}

export async function toggleMaximizeWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
    const maximized = await win.isMaximized();
    if (maximized) {
      await win.unmaximize();
    } else {
      await win.maximize();
    }
  } catch (err) {
    console.warn("toggleMaximizeWindow failed:", err);
  }
}

export async function toggleFullscreenWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
    const fullscreen = await win.isFullscreen();
    await win.setFullscreen(!fullscreen);
  } catch (err) {
    console.warn("toggleFullscreenWindow failed:", err);
  }
}

export async function onWindowFocusChange(
  handler: (focused: boolean) => void,
): Promise<() => void> {
  try {
    const win = getCurrentWindow();
    const unlisten = await win.onFocusChanged(({ payload }) => handler(payload));
    return unlisten;
  } catch (err) {
    console.warn("onWindowFocusChange subscription failed:", err);
    return () => {};
  }
}


export async function getLogicalWindowSize(): Promise<{ width: number; height: number } | null> {
  try {
    const win = getCurrentWindow();
    const size = await win.outerSize();
    const scale = await win.scaleFactor();
    return {
      width: Math.round(size.width / scale),
      height: Math.round(size.height / scale),
    };
  } catch (err) {
    console.warn("getLogicalWindowSize failed:", err);
    return null;
  }
}

export async function setLogicalWindowSize(width: number, height: number): Promise<void> {
  try {
    const win = getCurrentWindow();
    await win.setSize(new LogicalSize(width, height));
  } catch (err) {
    console.warn("setLogicalWindowSize failed:", err);
  }
}

export async function getLogicalWindowPosition(): Promise<{ x: number; y: number } | null> {
  try {
    const win = getCurrentWindow();
    const pos = await win.outerPosition();
    const scale = await win.scaleFactor();
    return {
      x: Math.round(pos.x / scale),
      y: Math.round(pos.y / scale),
    };
  } catch (err) {
    console.warn("getLogicalWindowPosition failed:", err);
    return null;
  }
}

export async function setLogicalWindowPosition(x: number, y: number): Promise<void> {
  try {
    const win = getCurrentWindow();
    await win.setPosition(new LogicalPosition(x, y));
  } catch (err) {
    console.warn("setLogicalWindowPosition failed:", err);
  }
}

export async function centerWindow(): Promise<void> {
  try {
    await getCurrentWindow().center();
  } catch (err) {
    console.warn("centerWindow failed:", err);
  }
}

export async function computeWindowPlacement(
  x: number,
  y: number,
  width: number,
  height: number,
): Promise<{ x: number; y: number } | null> {
  try {
    return await invoke("compute_window_placement", { x, y, width, height });
  } catch (err) {
    console.warn("computeWindowPlacement failed:", err);
    return null;
  }
}

export async function onWindowResized(handler: () => void): Promise<() => void> {
  try {
    const win = getCurrentWindow();
    const unlisten = await win.onResized(() => handler());
    return unlisten;
  } catch (err) {
    console.warn("onWindowResized subscription failed:", err);
    return () => {};
  }
}

export async function onWindowMoved(handler: () => void): Promise<() => void> {
  try {
    const win = getCurrentWindow();
    const unlisten = await win.onMoved(() => handler());
    return unlisten;
  } catch (err) {
    console.warn("onWindowMoved subscription failed:", err);
    return () => {};
  }
}

export async function onWindowCloseRequested(
  handler: () => Promise<void> | void,
): Promise<() => void> {
  try {
    const win = getCurrentWindow();
    let closing = false;
    const unlisten = await win.onCloseRequested(async (event) => {
      event.preventDefault();
      if (closing) return;
      closing = true;
      try {
        await handler();
      } catch (err) {
        console.warn("onWindowCloseRequested handler threw:", err);
      } finally {
        try {
          await win.destroy();
        } catch (err) {
          console.warn("onWindowCloseRequested destroy failed:", err);
        }
      }
    });
    return unlisten;
  } catch (err) {
    console.warn("onWindowCloseRequested subscription failed:", err);
    return () => {};
  }
}

// --- Preview surface (ADR-009, lean scope) ---

export interface PreviewRendererInfo {
  content_type: string;
  capabilities: {
    supports_live_render: boolean;
    supports_print: boolean;
    max_safe_document_bytes: number;
  };
}

export type PreviewRenderResult =
  | { kind: "rendered"; used_fallback_stylesheet: boolean; parser_warnings: string[] }
  | { kind: "no_renderer"; content_type: string }
  | { kind: "failed"; message: string };

export async function previewListRenderers(): Promise<PreviewRendererInfo[]> {
  return invoke("preview_list_renderers");
}

export async function previewRender(
  windowId: number,
  bufferId: string,
  contentType: string,
  text: string,
  theme: ThemePolarity,
  zoom: number,
): Promise<PreviewRenderResult> {
  return invoke("preview_render", { windowId, bufferId, contentType, text, theme, zoom });
}

export async function previewForceRender(
  windowId: number,
  bufferId: string,
  contentType: string,
  text: string,
  theme: ThemePolarity,
  zoom: number,
): Promise<PreviewRenderResult> {
  return invoke("preview_force_render", { windowId, bufferId, contentType, text, theme, zoom });
}

export async function previewClose(bufferId: string): Promise<void> {
  return invoke("preview_close", { bufferId });
}

export async function previewSetLayout(
  windowId: number,
  bufferId: string,
  path: string | null,
  layout: string,
  ratio: number | null,
): Promise<void> {
  return invoke("preview_set_layout", { windowId, bufferId, path, layout, ratio });
}

export interface PersistedLayout {
  layout: string;
  ratio: number | null;
}

export async function previewGetLayout(path: string): Promise<PersistedLayout | null> {
  return invoke("preview_get_layout", { path });
}


import type { WorkspaceEntry } from "../types/workspace";
export type { WorkspaceEntry };

export async function pickWorkspaceFolder(): Promise<string | null> {
  return invoke("pick_workspace_folder");
}

export async function clearWorkspaceRoot(): Promise<void> {
  return invoke("clear_workspace_root");
}

export async function listWorkspaceDir(dirPath: string): Promise<WorkspaceEntry[]> {
  return invoke("list_workspace_dir", { dirPath });
}

export async function getWorkspaceRoot(): Promise<string | null> {
  return invoke("get_workspace_root");
}

export interface InstallCliResult {
  symlink_path: string;
  manual_command: string;
}

export interface CliStatus {
  installed: boolean;
  path: string;
}

export async function cliStatus(): Promise<CliStatus> {
  return invoke("cli_status");
}

export async function installCli(): Promise<InstallCliResult> {
  return invoke("install_cli");
}

// --- Default app (macOS only) ---

export type DefaultAppStatus =
  | { status: "is_default" }
  | { status: "other_app"; name: string | null }
  | { status: "no_handler" }
  | { status: "unsupported" };

export interface ClaimableType {
  id: string;
  label: string;
  exts: string[];
  utis: string[];
}

export async function listDefaultAppTypes(): Promise<ClaimableType[]> {
  return invoke("list_default_app_types");
}

export async function getDefaultAppStatus(id: string): Promise<DefaultAppStatus> {
  return invoke("get_default_app_status", { id });
}

export async function setDefaultApp(id: string): Promise<void> {
  return invoke("set_default_app", { id });
}

// --- Watch inbox (ADR-018) ---

export async function pickInboxFolder(): Promise<string | null> {
  return invoke("pick_inbox_folder");
}

export async function clearInbox(): Promise<void> {
  return invoke("clear_inbox");
}

export async function getInboxPath(): Promise<string | null> {
  return invoke("get_inbox_path");
}

export interface InboxFile {
  name: string;
  path: string;
  size_bytes: number;
}

export async function listInboxFiles(): Promise<InboxFile[]> {
  return invoke("list_inbox_files");
}

// --- Storage location ---

export interface StorageInfo {
  db_path: string;
  dir: string;
}

export async function getStorageInfo(): Promise<StorageInfo> {
  return invoke("get_storage_info");
}

export async function revealStoragePath(): Promise<void> {
  return invoke("reveal_storage_path");
}

// --- Rewrite (opt-in) ---

export type AiAction = "proofread" | "rephrase" | "polish" | "custom";

export interface AiKeyState {
  is_set: boolean;
  memory_only: boolean;
}

export async function aiRewrite(
  requestId: string,
  action: AiAction,
  text: string,
  customInstruction?: string,
): Promise<string> {
  return invoke("ai_rewrite", {
    requestId,
    action,
    text,
    customInstruction: customInstruction ?? null,
  });
}

export async function aiCancel(requestId: string): Promise<void> {
  return invoke("ai_cancel", { requestId });
}

export interface AiConnectionStatus {
  reachable: boolean;
  model_listed: boolean | null;
  kind: string;
  detail: string;
}

export async function aiCheckConnection(): Promise<AiConnectionStatus> {
  return invoke("ai_check_connection");
}

export async function aiSetApiKey(preset: string, key: string): Promise<AiKeyState> {
  return invoke("ai_set_api_key", { preset, key });
}

export async function aiClearApiKey(preset: string): Promise<AiKeyState> {
  return invoke("ai_clear_api_key", { preset });
}

export async function aiHasApiKey(preset: string): Promise<AiKeyState> {
  return invoke("ai_has_api_key", { preset });
}

export async function showAndFocusWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
    if (await win.isMinimized()) {
      await win.unminimize();
    }
    await win.show();
    await win.setFocus();
  } catch (err) {
    console.warn("showAndFocusWindow failed:", err);
  }
}
