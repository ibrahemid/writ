import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize, LogicalPosition } from "@tauri-apps/api/window";
import type { BufferDocument } from "../types/buffer";
import type { WritConfig } from "../types/config";
import type { TransformDescriptor } from "../types/transforms";

export async function listTransforms(): Promise<TransformDescriptor[]> {
  return invoke("list_transforms");
}

export async function applyTransform(transformId: string, input: string): Promise<string> {
  return invoke("apply_transform", { transformId, input });
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
  return invoke("read_buffer_content", { id });
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

export async function searchBuffers(query: string): Promise<string[]> {
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

export async function openFile(path: string): Promise<BufferDocument> {
  return invoke("open_file", { path });
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
): Promise<PreviewRenderResult> {
  return invoke("preview_render", { windowId, bufferId, contentType, text });
}

export async function previewForceRender(
  windowId: number,
  bufferId: string,
  contentType: string,
  text: string,
): Promise<PreviewRenderResult> {
  return invoke("preview_force_render", { windowId, bufferId, contentType, text });
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

