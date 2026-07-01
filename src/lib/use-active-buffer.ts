import { createMemo, type Accessor } from "solid-js";
import { bufferRegistry } from "../stores/global/buffer-registry";
import { useWindow } from "../components/WindowProvider/WindowProvider";
import type { BufferDocument } from "../types/buffer";

// The active buffer for the current window: the loaded buffer matching the
// active tab id, or null when there is no active tab. Must be called within a
// WindowProvider (it reads the window context).
export function useActiveBuffer(): Accessor<BufferDocument | null> {
  const win = useWindow();
  return createMemo(() => {
    const id = win.tabs.activeTabId();
    if (!id) return null;
    return bufferRegistry.activeTabs().find((b) => b.id === id) ?? null;
  });
}
