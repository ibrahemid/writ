import { createSignal } from "solid-js";

// Per-window active layout per buffer. Phase 1 ships the type; Phase 2 wires
// LayoutMode persistence to writ-storage (migration 010_layout_state.sql) and
// the layout cycle keymap. Per-buffer ratio survives reload via the layout-
// state table.

export type LayoutMode =
  | { kind: "source" }
  | { kind: "preview" }
  | { kind: "split"; ratio: number; orientation: "vertical" | "horizontal" }
  | { kind: "detached"; windowId: number };

export type LayoutStore = ReturnType<typeof createLayoutStore>;

const DEFAULT_LAYOUT: LayoutMode = { kind: "source" };

export function createLayoutStore() {
  const layouts = new Map<string, LayoutMode>();
  const previousLayouts = new Map<string, LayoutMode>();
  const [version, setVersion] = createSignal(0);

  function bump() {
    setVersion((v) => v + 1);
  }

  function get(bufferId: string): LayoutMode {
    void version();
    return layouts.get(bufferId) ?? DEFAULT_LAYOUT;
  }

  function set(bufferId: string, layout: LayoutMode): void {
    const prior = layouts.get(bufferId);
    if (prior) previousLayouts.set(bufferId, prior);
    layouts.set(bufferId, layout);
    bump();
  }

  function restorePrevious(bufferId: string): LayoutMode {
    const prior = previousLayouts.get(bufferId);
    if (!prior) return DEFAULT_LAYOUT;
    layouts.set(bufferId, prior);
    previousLayouts.delete(bufferId);
    bump();
    return prior;
  }

  function clear(bufferId: string): void {
    layouts.delete(bufferId);
    previousLayouts.delete(bufferId);
    bump();
  }

  return { get, set, restorePrevious, clear };
}
