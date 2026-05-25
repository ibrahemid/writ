import { createSignal, createRoot } from "solid-js";

// Singleton — app-global, not window-scoped (ADR-009 E3).
// Mirrors the Rust ContentRendererRegistry. Populated at startup from
// preview_list_renderers IPC; consumed by PreviewLayout and the policy chip.
// Phase 1 ships the type; Phase 2 wires the HTML renderer; later phases add
// the rest of the roster.

export interface RendererInfo {
  contentType: string;
  defaultLayout: "source" | "preview" | "split";
  supportsLiveRender: boolean;
  supportsPrint: boolean;
  maxSafeDocumentBytes: number;
}

function createRendererRegistry() {
  const [renderers, setRenderers] = createSignal<RendererInfo[]>([]);

  function setAll(list: RendererInfo[]): void {
    setRenderers(list);
  }

  function get(contentType: string): RendererInfo | null {
    return renderers().find((r) => r.contentType === contentType) ?? null;
  }

  function hasRenderer(contentType: string): boolean {
    return get(contentType) !== null;
  }

  return { renderers, setAll, get, hasRenderer };
}

export const rendererRegistry = createRoot(createRendererRegistry);
