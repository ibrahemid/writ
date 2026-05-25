import { createSignal, createRoot } from "solid-js";
// Type-only import — see store-layer-boundary.test.ts: type-only crossings
// carry no runtime cost and are exempt from the global↔window boundary.
import type { WindowState } from "../window/createWindowState";

// Singleton — app-global, not window-scoped (ADR-009 E3).
// Tracks per-window state instances so app-level commands can resolve "the
// active window" without needing context access. Each <WindowProvider>
// registers its state on mount and reports focus transitions.

export type WindowId = number;

function createWindowRegistry() {
  const all = new Map<WindowId, WindowState>();
  const [active, setActive] = createSignal<WindowState | null>(null);

  function register(state: WindowState): () => void {
    all.set(state.windowId, state);
    if (!active()) setActive(state);
    return () => unregister(state.windowId);
  }

  function unregister(windowId: WindowId): void {
    all.delete(windowId);
    if (active()?.windowId === windowId) {
      const next = all.values().next().value ?? null;
      setActive(next);
    }
  }

  function focus(windowId: WindowId): void {
    const state = all.get(windowId);
    if (state) setActive(state);
  }

  function getActive(): WindowState | null {
    return active();
  }

  function activeWindow() {
    return active();
  }

  function size(): number {
    return all.size;
  }

  return { register, unregister, focus, getActive, activeWindow, size };
}

export const windowRegistry = createRoot(createWindowRegistry);
