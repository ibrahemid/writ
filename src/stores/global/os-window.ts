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
  let geometryTimer: ReturnType<typeof setTimeout> | null = null;

  async function installFocusSync(): Promise<() => void> {
    return api.onWindowFocusChange(setFocused);
  }

  async function persistGeometryNow(): Promise<void> {
    const size = await api.getLogicalWindowSize();
    if (!size) return;
    const pos = await api.getLogicalWindowPosition();
    const existing = configStore.config().window;
    const next = {
      width: size.width,
      height: size.height,
      x: pos ? pos.x : (existing?.x ?? null),
      y: pos ? pos.y : (existing?.y ?? null),
    };
    if (
      existing &&
      existing.width === next.width &&
      existing.height === next.height &&
      (existing.x ?? null) === next.x &&
      (existing.y ?? null) === next.y
    ) {
      return;
    }
    try {
      await configStore.save({ ...configStore.config(), window: next });
    } catch {}
  }

  function scheduleGeometryPersist(): void {
    if (geometryTimer) clearTimeout(geometryTimer);
    geometryTimer = setTimeout(() => {
      geometryTimer = null;
      void persistGeometryNow();
    }, PERSIST_DEBOUNCE_MS);
  }

  // Cancel any debounced write and persist the current geometry now, so a quit
  // within the debounce window cannot lose the last move/resize.
  async function flushGeometry(): Promise<void> {
    if (geometryTimer) {
      clearTimeout(geometryTimer);
      geometryTimer = null;
    }
    await persistGeometryNow();
  }

  async function installGeometryPersistence(): Promise<() => void> {
    const unResized = await api.onWindowResized(scheduleGeometryPersist);
    const unMoved = await api.onWindowMoved(scheduleGeometryPersist);
    return () => {
      if (geometryTimer) {
        clearTimeout(geometryTimer);
        geometryTimer = null;
      }
      unResized();
      unMoved();
    };
  }

  return {
    focused,
    installFocusSync,
    installGeometryPersistence,
    flushGeometry,
    reveal: api.showWindow,
    hide: api.hideWindow,
    minimize: api.minimizeWindow,
    toggleMaximize: api.toggleMaximizeWindow,
    toggleFullscreen: api.toggleFullscreenWindow,
    startDragging: api.startDraggingWindow,
  };
}

export const osWindowStore = createRoot(createOsWindowStore);
