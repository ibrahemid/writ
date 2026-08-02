import { createSignal, createRoot } from "solid-js";
import * as api from "../../services/tauri";
import { configStore } from "./config";

// Singleton — app-global, not window-scoped (ADR-009 E3).
// OS window chrome (focus, drag, min, max, hide, size) is per-OS-window by
// construction: getCurrentWindow() in each frontend instance resolves to that
// instance's window. The singleton is correct because each frontend root has
// exactly one of these.

const PERSIST_DEBOUNCE_MS = 500;
const MAXIMIZED_COALESCE_MS = 100;

function createOsWindowStore() {
  const [focused, setFocused] = createSignal(true);
  const [maximized, setMaximized] = createSignal(false);
  let geometryTimer: ReturnType<typeof setTimeout> | null = null;
  let maximizedTimer: ReturnType<typeof setTimeout> | null = null;

  async function installFocusSync(): Promise<() => void> {
    return api.onWindowFocusChange(setFocused);
  }

  async function syncMaximized(): Promise<void> {
    setMaximized(await api.isWindowMaximized());
  }

  // Dragging a window edge emits resize continuously, and each one would cost an
  // IPC round-trip. The button's own path re-reads immediately (see below), so
  // this listener only has to catch OS-initiated changes: snap layouts, Win+Up,
  // drag-to-top. A short trailing coalesce keeps those prompt without the storm.
  function scheduleMaximizedSync(): void {
    if (maximizedTimer) clearTimeout(maximizedTimer);
    maximizedTimer = setTimeout(() => {
      maximizedTimer = null;
      void syncMaximized();
    }, MAXIMIZED_COALESCE_MS);
  }

  async function installMaximizeSync(): Promise<() => void> {
    const unResized = await api.onWindowResized(scheduleMaximizedSync);
    // Seeded off the cold path: the window is still hidden here and starts
    // restored, so this must not add a round-trip ahead of the first paint.
    scheduleMaximizedSync();
    return () => {
      if (maximizedTimer) {
        clearTimeout(maximizedTimer);
        maximizedTimer = null;
      }
      unResized();
    };
  }

  // Re-read as soon as the toggle resolves rather than waiting on the coalesced
  // resize: a window maximized to the same bounds it already had emits no
  // resize at all, which would leave the button showing the state just left.
  async function toggleMaximize(): Promise<void> {
    await api.toggleMaximizeWindow();
    await syncMaximized();
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
    } catch (err) {
      console.error("[osWindowStore] failed to persist window geometry", err);
    }
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
    maximized,
    installFocusSync,
    installMaximizeSync,
    installGeometryPersistence,
    flushGeometry,
    reveal: api.showWindow,
    hide: api.hideWindow,
    minimize: api.minimizeWindow,
    toggleMaximize,
    toggleFullscreen: api.toggleFullscreenWindow,
    startDragging: api.startDraggingWindow,
  };
}

export const osWindowStore = createRoot(createOsWindowStore);
