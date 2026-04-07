import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, type DragDropEvent } from "@tauri-apps/api/window";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { BufferDocument } from "../types/buffer";
import type { WritConfig } from "../types/config";

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

export async function consumePendingOpens(): Promise<string[]> {
  return invoke("consume_pending_opens");
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

export async function hideWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
    await win.hide();
  } catch {}
}

export async function minimizeWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
    await win.minimize();
  } catch {}
}

export async function onDragDrop(
  handler: (event: DragDropEvent) => void,
): Promise<() => void> {
  const win = getCurrentWindow();
  return win.onDragDropEvent((event) => {
    handler(event.payload);
  });
}

