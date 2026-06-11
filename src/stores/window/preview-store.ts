import { createSignal } from "solid-js";
import {
  previewClose,
  previewForceRender,
  type PreviewRenderResult,
} from "../../services/tauri";
import type { PreviewSearchController } from "../../editor/search/preview-search-controller";

// Per-window preview coordination. Lean scope: the preview is a same-window
// iframe, so there is no per-pane webview process to pool, idle-pause, or
// crash-count. This store owns the render/close IPC for the window's preview
// panes and a force-refresh signal: the F5 keymap bumps it; the active
// PreviewPane watches it and re-renders past the size-threshold debounce gate.

export type PreviewStore = ReturnType<typeof createPreviewStore>;

export function createPreviewStore(deps: { windowId: number }) {
  const { windowId } = deps;
  const [forceRefreshToken, setForceRefreshToken] = createSignal(0);
  // The preview pane's search controller, registered only while a renderable
  // buffer is shown preview-only (the editor hidden). find-store routes to it
  // when present; null means find falls back to the editor.
  const [searchController, setSearchController] =
    createSignal<PreviewSearchController | null>(null);

  function requestForceRefresh(): void {
    setForceRefreshToken((t) => t + 1);
  }

  /** Register (or clear) the active preview search controller. */
  function registerSearch(controller: PreviewSearchController | null): void {
    setSearchController(controller);
  }

  /** The preview search controller when the preview owns find, else null. */
  function activeSearch(): PreviewSearchController | null {
    return searchController();
  }

  /** Render the live buffer text and cache the HTML for the iframe to load. */
  async function render(
    bufferId: string,
    contentType: string,
    text: string,
  ): Promise<PreviewRenderResult> {
    return previewForceRender(windowId, bufferId, contentType, text);
  }

  /** Drop the host-side render cache entry when a pane unmounts. */
  function close(bufferId: string): void {
    void previewClose(bufferId).catch(() => {});
  }

  return {
    forceRefreshToken,
    requestForceRefresh,
    registerSearch,
    activeSearch,
    render,
    close,
  };
}
