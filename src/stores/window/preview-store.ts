import { createSignal } from "solid-js";
import {
  previewClose,
  previewForceRender,
  type PreviewRenderResult,
} from "../../services/tauri";
import type { ThemePolarity } from "../../types/theme";
import type { PreviewSearchController } from "../../editor/search/preview-search-controller";
import { previewScriptsStore } from "../global/preview-scripts";
import { leadingCodeOf } from "../../lib/read-failure";
import { logFailure } from "../../lib/log";

/** A render whose work stopped before it answered (ADR-047). */
const ERR_COMMAND_STOPPED = "ERR_COMMAND_STOPPED";

// Per-window preview coordination. Lean scope: the preview is a same-window
// iframe, so there is no per-pane webview process to pool, idle-pause, or
// crash-count. This store owns the render/close IPC for the window's preview
// panes and a force-refresh signal: the F5 keymap bumps it; the active
// PreviewPane watches it and re-renders past the size-threshold debounce gate.

export type PreviewStore = ReturnType<typeof createPreviewStore>;

export function createPreviewStore() {
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

  /**
   * Render the live buffer text and cache the HTML for the iframe to load. The
   * render checks the file's scripts opt-in, so its verdict is what the status
   * bar shows from then on. A render whose work stopped answers as a failed
   * render, which the pane shows in place of the document.
   */
  async function render(
    bufferId: string,
    contentType: string,
    text: string,
    theme: ThemePolarity,
    zoom: number,
  ): Promise<PreviewRenderResult> {
    const issuedAt = previewScriptsStore.issue();
    let result: PreviewRenderResult;
    try {
      result = await previewForceRender(bufferId, contentType, text, theme, zoom);
    } catch (error) {
      if (leadingCodeOf(error, [ERR_COMMAND_STOPPED]) === null) throw error;
      logFailure("a preview render stopped before it answered");
      return {
        kind: "failed",
        message: "The render stopped before it finished. Run Preview: Refresh to try again.",
      };
    }
    if (result.kind === "rendered") {
      previewScriptsStore.settle(bufferId, issuedAt, result.scripts_enabled);
    }
    return result;
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
