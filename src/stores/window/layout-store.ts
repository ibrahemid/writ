import { createSignal } from "solid-js";
import {
  layoutFromPersisted,
  layoutRatio,
  markdownDefaultLayout,
  type LayoutMode,
} from "../../lib/preview-layout";
import { configStore } from "../global/config";
import { previewGetLayout, previewSetLayout } from "../../services/tauri";

// Per-window active layout per buffer. The pure types + helpers live
// in lib/preview-layout so components and keymap can reach them without
// crossing the store layer; this store owns runtime per-window state and
// persistence to writ-storage via the preview IPC.

export type LayoutStore = ReturnType<typeof createLayoutStore>;

const DEFAULT_LAYOUT: LayoutMode = { kind: "source" };

export function createLayoutStore(deps: { windowId: number }) {
  const { windowId } = deps;
  const layouts = new Map<string, LayoutMode>();
  const previousLayouts = new Map<string, LayoutMode>();
  const [version, setVersion] = createSignal(0);

  function bump() {
    setVersion((v) => v + 1);
  }

  /**
   * The buffer's layout. A markdown buffer nothing has resolved yet answers
   * the configured default rather than source: the editor mounts with this,
   * and resolving it a round trip later is a flash of raw markup.
   */
  function get(bufferId: string, contentType: string | null): LayoutMode {
    void version();
    const resolved = layouts.get(bufferId);
    if (resolved) return resolved;
    if (contentType === "markdown") {
      return markdownDefaultLayout(configStore.config().preview.default_layout_markdown);
    }
    return DEFAULT_LAYOUT;
  }

  function setLocal(bufferId: string, layout: LayoutMode): void {
    const prior = layouts.get(bufferId);
    if (prior && prior.kind !== layout.kind) previousLayouts.set(bufferId, prior);
    layouts.set(bufferId, layout);
    bump();
  }

  /** Set the layout in memory and persist it (scratch buffers — null path — are not persisted). */
  function set(bufferId: string, path: string | null, layout: LayoutMode): void {
    setLocal(bufferId, layout);
    void previewSetLayout(windowId, bufferId, path, layout.kind, layoutRatio(layout)).catch(
      () => {},
    );
  }

  /** Load a source-backed buffer's persisted layout, if any. */
  async function hydrate(path: string, contentType: string | null): Promise<LayoutMode | null> {
    try {
      const persisted = await previewGetLayout(path);
      if (!persisted) return null;
      return layoutFromPersisted(persisted.layout, persisted.ratio, contentType);
    } catch {
      return null;
    }
  }

  /** Restore the layout in effect before the last kind change (exit fullscreen). */
  function restorePrevious(bufferId: string, path: string | null): LayoutMode {
    const prior = previousLayouts.get(bufferId) ?? DEFAULT_LAYOUT;
    previousLayouts.delete(bufferId);
    set(bufferId, path, prior);
    return prior;
  }

  function clear(bufferId: string): void {
    layouts.delete(bufferId);
    previousLayouts.delete(bufferId);
    bump();
  }

  return { get, set, setLocal, hydrate, restorePrevious, clear };
}
