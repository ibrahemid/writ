import { createRoot, createSignal } from "solid-js";
import * as api from "../../services/tauri";
import { logFailure } from "../../lib/log";
import { togglePreviewScripts } from "../../lib/preview-actions";
import { windowRegistry } from "./window-registry";

// Singleton state — Writ is single-window. Each file's preview-scripts opt-in
// as the store holds it (decision RC-D1/TS-D1), so what the status bar shows
// is the stored choice: read when the field shows a file, and set again by the
// toggle and by a change another program makes to the file (ADR-043).

export function createPreviewScriptsStore() {
  const [known, setKnown] = createSignal<ReadonlyMap<string, boolean>>(new Map());

  function record(bufferId: string, enabled: boolean): void {
    setKnown((previous) => new Map(previous).set(bufferId, enabled));
  }

  /** The file's stored opt-in, or undefined until it has been read. */
  function enabled(bufferId: string): boolean | undefined {
    return known().get(bufferId);
  }

  /** Read the file's stored opt-in. A read that fails leaves it unknown. */
  async function load(bufferId: string): Promise<void> {
    try {
      record(bufferId, await api.previewGetScripts(bufferId));
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
    const next = await togglePreviewScripts(bufferId, enabled(bufferId));
    record(bufferId, next);
    return next;
  }

  /**
   * Turn the file's scripts off because another program changed it, and reload
   * the preview showing it, which may still be running the document the opt-in
   * was given to.
   */
  async function turnOff(bufferId: string): Promise<void> {
    try {
      await api.previewSetScripts(bufferId, false);
      record(bufferId, false);
    } catch {
      logFailure("a changed file's preview scripts could not be turned off");
      await load(bufferId);
    }
    const win = windowRegistry.getActive();
    if (win && win.tabs.activeTabId() === bufferId) win.preview.requestForceRefresh();
  }

  return { enabled, load, toggle, turnOff };
}

export const previewScriptsStore = createRoot(createPreviewScriptsStore);
