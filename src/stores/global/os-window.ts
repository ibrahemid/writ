import { createSignal, createRoot } from "solid-js";
import * as api from "../../services/tauri";
import { onEvent } from "../../services/events";
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
  const [snapHovered, setSnapHovered] = createSignal(false);
  const [snapPressed, setSnapPressed] = createSignal(false);
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
    // Seeded off the cold path: this runs before the window is revealed (and
    // before a saved maximized state is reapplied), so it must not add a
    // round-trip ahead of the first paint. The reapply emits a resize, which
    // the listener above turns into the seeding read.
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

  // On Windows the maximize button is covered by the child window that answers
  // the snap-layout hit test, so the button itself never receives a real mouse
  // event: CSS :hover and :active are dead over it and the pointer state has to
  // come back from Rust.
  async function installSnapOverlay(metrics: api.CaptionButtonMetrics): Promise<() => void> {
    await api.reportCaptionButtonMetrics(metrics);
    const unlisten = await onEvent("titlebar:maximize-hit", ({ phase }) => {
      switch (phase) {
        case "enter":
          setSnapHovered(true);
          break;
        case "leave":
          setSnapHovered(false);
          setSnapPressed(false);
          break;
        case "press":
          setSnapPressed(true);
          break;
        case "click":
          // The pointer is still over the button, so the hover stands.
          setSnapPressed(false);
          void toggleMaximize();
          break;
      }
    });
    return () => {
      unlisten();
      setSnapHovered(false);
      setSnapPressed(false);
    };
  }

  async function persistGeometryNow(): Promise<void> {
    // A minimized window reports garbage bounds (-32000 on Windows) and a
    // fullscreen one reports the screen; neither is the geometry to restore to.
    if (await api.isWindowMinimized()) return;
    if (await api.isWindowFullscreen()) return;

    const existing = configStore.config().window;

    // While maximized the window reports the work area, not the rect the user
    // sized, so the stored width/height stay the floating ones the next launch
    // unmaximizes back to. The position is still worth taking: it tracks the
    // monitor the maximized window sits on, and place_window clamps it into
    // that monitor's work area on restore.
    if (await api.isWindowMaximized()) {
      const maxPos = await api.getLogicalWindowPosition();
      const x = maxPos ? maxPos.x : (existing.x ?? null);
      const y = maxPos ? maxPos.y : (existing.y ?? null);
      if (existing.maximized && (existing.x ?? null) === x && (existing.y ?? null) === y) return;
      try {
        await configStore.save({
          ...configStore.config(),
          window: { ...existing, x, y, maximized: true },
        });
      } catch (err) {
        console.error("[osWindowStore] failed to persist window geometry", err);
      }
      return;
    }

    const size = await api.getLogicalWindowSize();
    if (!size) return;
    const pos = await api.getLogicalWindowPosition();
    const next = {
      width: size.width,
      height: size.height,
      x: pos ? pos.x : (existing.x ?? null),
      y: pos ? pos.y : (existing.y ?? null),
      maximized: false,
    };
    if (
      existing &&
      existing.width === next.width &&
      existing.height === next.height &&
      (existing.x ?? null) === next.x &&
      (existing.y ?? null) === next.y &&
      existing.maximized === next.maximized
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
    snapHovered,
    snapPressed,
    installFocusSync,
    installMaximizeSync,
    installSnapOverlay,
    installGeometryPersistence,
    flushGeometry,
    reveal: api.showWindow,
    hide: api.hideWindow,
    minimize: api.minimizeWindow,
    maximize: api.maximizeWindow,
    toggleMaximize,
    toggleFullscreen: api.toggleFullscreenWindow,
    startDragging: api.startDraggingWindow,
  };
}

export const osWindowStore = createRoot(createOsWindowStore);
