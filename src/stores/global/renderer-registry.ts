import { createSignal, createRoot } from "solid-js";
import type { PreviewRendererInfo } from "../../services/tauri";

// Singleton — app-global, not window-scoped (ADR-009 E3).
// Mirrors the Rust ContentRendererRegistry. Populated at startup from
// previewListRenderers(); consumed by the layout resolver and PreviewPane to
// decide whether a content type is renderable.

export interface RendererInfo {
  contentType: string;
  supportsLiveRender: boolean;
  supportsPrint: boolean;
  maxSafeDocumentBytes: number;
}

function fromIpc(info: PreviewRendererInfo): RendererInfo {
  return {
    contentType: info.content_type,
    supportsLiveRender: info.capabilities.supports_live_render,
    supportsPrint: info.capabilities.supports_print,
    maxSafeDocumentBytes: info.capabilities.max_safe_document_bytes,
  };
}

function createRendererRegistry() {
  const [renderers, setRenderers] = createSignal<RendererInfo[]>([]);

  function setFromIpc(list: PreviewRendererInfo[]): void {
    setRenderers(list.map(fromIpc));
  }

  function get(contentType: string | null): RendererInfo | null {
    if (contentType === null) return null;
    return renderers().find((r) => r.contentType === contentType) ?? null;
  }

  function hasRenderer(contentType: string | null): boolean {
    return get(contentType) !== null;
  }

  return { renderers, setFromIpc, get, hasRenderer };
}

export const rendererRegistry = createRoot(createRendererRegistry);
