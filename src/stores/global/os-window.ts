import { createSignal, createRoot } from "solid-js";
import * as api from "../../services/tauri";
import { configStore } from "./config";

// Singleton — app-global, not window-scoped (ADR-009 E3).
// OS window chrome (focus, drag, min, max, hide, size) is per-OS-window by
// construction: getCurrentWindow() in each frontend instance resolves to that
// instance's window. The singleton is correct because each frontend root has
// exactly one of these.

const PERSIST_DEBOUNCE_MS = 500;

function createOsWindowStore() {
  const [focused, setFocused] = createSignal(true);
  let resizeTimer: ReturnType<typeof setTimeout> | null = null;

  async function installFocusSync(): Promise<() => void> {
    return api.onWindowFocusChange(setFocused);
  }

  async function restoreSize(): Promise<void> {
    const cfg = configStore.config().window;
    if (!cfg || cfg.width <= 0 || cfg.height <= 0) return;
    const current = await api.getLogicalWindowSize();
    if (current && current.width === cfg.width && current.height === cfg.height) return;
    await api.setLogicalWindowSize(cfg.width, cfg.height);
  }

  async function installSizePersistence(): Promise<() => void> {
    const unlisten = await api.onWindowResized(() => {
      if (resizeTimer) clearTimeout(resizeTimer);
      resizeTimer = setTimeout(async () => {
        const current = await api.getLogicalWindowSize();
        if (!current) return;
        const existing = configStore.config().window;
        if (existing && existing.width === current.width && existing.height === current.height) {
          return;
        }
        try {
          await configStore.save({
            ...configStore.config(),
            window: { width: current.width, height: current.height },
          });
        } catch {}
      }, PERSIST_DEBOUNCE_MS);
    });
    return () => {
      if (resizeTimer) {
        clearTimeout(resizeTimer);
        resizeTimer = null;
      }
      unlisten();
    };
  }

  return {
    focused,
    installFocusSync,
    restoreSize,
    installSizePersistence,
    hide: api.hideWindow,
    minimize: api.minimizeWindow,
    toggleMaximize: api.toggleMaximizeWindow,
    startDragging: api.startDraggingWindow,
  };
}

export const osWindowStore = createRoot(createOsWindowStore);
