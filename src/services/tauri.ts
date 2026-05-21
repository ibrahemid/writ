import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, type DragDropEvent } from "@tauri-apps/api/window";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
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
  const selected = await openDialog({
    multiple: false,
    title: "Open File",
  });
  if (typeof selected === "string") return selected;
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

export async function hideWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
    await win.hide();
  } catch (err) {
    console.warn("hideWindow failed:", err);
  }
}

export async function minimizeWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
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

export async function onDragDrop(
  handler: (event: DragDropEvent) => void,
): Promise<() => void> {
  const win = getCurrentWindow();
  return win.onDragDropEvent((event) => {
    handler(event.payload);
  });
}

