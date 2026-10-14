import { createRoot, createSignal } from "solid-js";
import * as api from "../../services/tauri";
import { logFailure } from "../../lib/log";
import { windowRegistry } from "./window-registry";

// Singleton state — Writ is single-window. Each file's preview-scripts opt-in
// as the backend last answered it (decision RC-D1/TS-D1), so what the status
// bar shows is what the preview runs: read when the field shows a file, set by
// the toggle, by every render's verdict, and read again when another program
// changes the file and the backend lets the opt-in lapse (ADR-043).

export function createPreviewScriptsStore() {
  const [known, setKnown] = createSignal<ReadonlyMap<string, boolean>>(new Map());
  // Answers land in the order they were asked for, not the order they arrive:
  // a read issued before a render must not overwrite the render's verdict.
  let asked = 0;
  const settledAt = new Map<string, number>();

  /** Mark a question about the file's opt-in as asked; answer with `settle`. */
  function issue(): number {
    asked += 1;
    return asked;
  }

  /**
   * Record the backend's answer to a question issued at `issuedAt`, unless an
   * answer to one asked later has already landed.
   */
  function settle(bufferId: string, issuedAt: number, enabled: boolean): void {
    if (issuedAt < (settledAt.get(bufferId) ?? 0)) return;
    settledAt.set(bufferId, issuedAt);
    setKnown((previous) => new Map(previous).set(bufferId, enabled));
  }

  /** The file's opt-in, or undefined until it has been read. */
  function enabled(bufferId: string): boolean | undefined {
    return known().get(bufferId);
  }

  /** Read the file's opt-in. A read that fails leaves what was known. */
  async function load(bufferId: string): Promise<void> {
    const issuedAt = issue();
    try {
      settle(bufferId, issuedAt, await api.previewGetScripts(bufferId));
    } catch {
      logFailure("a file's preview scripts setting could not be read");
    }
  }

  /**
   * Turn the file's scripts on if the status bar shows them off and off if it
   * shows them on, reading the stored choice for a file it has not shown, and
   * answer the new state. A choice that could not be recorded rejects, for the
   * caller to report.
   */
  async function toggle(bufferId: string): Promise<boolean> {
    const issuedAt = issue();
    const shown = enabled(bufferId);
    const next = !(shown ?? (await api.previewGetScripts(bufferId)));
    await api.previewSetScripts(bufferId, next);
    settle(bufferId, issuedAt, next);
    return next;
  }

  /**
   * Read the opt-in again after another program changed the file, which the
   * backend has already let lapse, and reload the preview showing it so the
   * document is served again under what the opt-in is now. The iframe only
   * navigates; it is never unmounted.
   */
  function refresh(bufferId: string): void {
    void load(bufferId);
    const win = windowRegistry.getActive();
    if (win && win.tabs.activeTabId() === bufferId) win.preview.requestForceRefresh();
  }

  return { enabled, issue, settle, load, toggle, refresh };
}

export const previewScriptsStore = createRoot(createPreviewScriptsStore);
