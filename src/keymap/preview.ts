import { registerCommand } from "../commands/registry";
import { bufferRegistry } from "../stores/global/buffer-registry";
import { windowRegistry } from "../stores/global/window-registry";
import { contentTypeForBuffer } from "../lib/content-type";
import { toggleRunScripts } from "../lib/preview-actions";
import {
  DEFAULT_RATIO,
  defaultSplit,
  nextCycleLayout,
  type LayoutMode,
} from "../lib/preview-layout";

// Preview keymap (lean scope). Detach is cut, so its binding is gone. The
// ADR-009 force-render binding (Cmd+R) is taken by note.rename today; L2
// ships preview-refresh on F5 and the rename rebind is a follow-up.

function activeWindow() {
  return windowRegistry.getActive();
}

function activeBufferId(): string | null {
  return activeWindow()?.tabs.activeTabId() ?? null;
}

function activeBufferOf(bufferId: string) {
  return bufferRegistry.activeTabs().find((b) => b.id === bufferId) ?? null;
}

function bufferPath(bufferId: string): string | null {
  return activeBufferOf(bufferId)?.source_path ?? null;
}

function contentTypeOf(bufferId: string): string | null {
  const buf = activeBufferOf(bufferId);
  return buf ? contentTypeForBuffer(buf) : null;
}

/** The split commands have nothing to act on where a file has no split. */
function hasSplitLayouts(bufferId: string): boolean {
  return contentTypeOf(bufferId) !== "markdown";
}

/** Register the preview keymap + the run-scripts kill switch palette entry. */
export function registerPreviewKeymap(): void {
  registerCommand({
    id: "preview.cycleLayout",
    label: "Preview: Cycle layout",
    description: "Switch how the file is shown",
    keybinding: "CmdOrCtrl+Shift+V",
    scope: "app",
    global: true,
    execute: () => {
      const w = activeWindow();
      const id = activeBufferId();
      if (!w || !id) return;
      const contentType = contentTypeOf(id);
      const next = nextCycleLayout(w.layout.get(id, contentType), contentType);
      w.layout.set(id, bufferPath(id), next);
    },
  });

  registerCommand({
    id: "preview.refresh",
    label: "Preview: Refresh",
    description: "Force a fresh render of the preview pane",
    keybinding: "F5",
    scope: "app",
    global: true,
    execute: () => activeWindow()?.preview.requestForceRefresh(),
  });

  registerCommand({
    id: "preview.toggleFullscreen",
    label: "Preview: Toggle fullscreen",
    description: "Show the preview pane only / return to split",
    keybinding: "CmdOrCtrl+Shift+R",
    scope: "app",
    global: true,
    execute: () => {
      const w = activeWindow();
      const id = activeBufferId();
      if (!w || !id || !hasSplitLayouts(id)) return;
      const current = w.layout.get(id, contentTypeOf(id));
      const path = bufferPath(id);
      if (current.kind === "preview") {
        w.layout.restorePrevious(id, path);
      } else {
        w.layout.set(id, path, { kind: "preview" });
      }
    },
  });

  registerCommand({
    id: "preview.exitFullscreen",
    label: "Preview: Exit fullscreen",
    description: "Return from fullscreen preview to the prior layout",
    keybinding: "Escape",
    scope: "app",
    global: true,
    // With no fullscreen preview to leave, Escape declines so it reaches the
    // focused editor, where Escape then Tab moves focus out (tab focus mode).
    execute: () => {
      const w = activeWindow();
      const id = activeBufferId();
      if (!w || !id || !hasSplitLayouts(id)) return false;
      if (w.layout.get(id, contentTypeOf(id)).kind !== "preview") return false;
      w.layout.restorePrevious(id, bufferPath(id));
    },
  });

  registerCommand({
    id: "preview.swapOrientation",
    label: "Preview: Swap split orientation",
    description: "Toggle vertical / horizontal split",
    // Moved off CmdOrCtrl+Shift+\, which the panel beside the note now takes
    // as the partner of the sidebar's CmdOrCtrl+\.
    keybinding: "CmdOrCtrl+Shift+H",
    scope: "app",
    global: true,
    execute: () => {
      const w = activeWindow();
      const id = activeBufferId();
      if (!w || !id) return;
      const current = w.layout.get(id, contentTypeOf(id));
      if (current.kind !== "split") return;
      const swapped: LayoutMode = {
        ...current,
        orientation: current.orientation === "vertical" ? "horizontal" : "vertical",
      };
      w.layout.set(id, bufferPath(id), swapped);
    },
  });

  registerCommand({
    id: "preview.resetRatio",
    label: "Preview: Reset split ratio",
    description: "Reset the split divider to 50/50",
    // Cmd+0 is the conventional "reset zoom" chord (editor.zoomReset owns it);
    // this stays on the preview family's Cmd+Shift+… prefix like its siblings.
    keybinding: "CmdOrCtrl+Shift+0",
    scope: "app",
    global: true,
    execute: () => {
      const w = activeWindow();
      const id = activeBufferId();
      if (!w || !id || !hasSplitLayouts(id)) return;
      const current = w.layout.get(id, contentTypeOf(id));
      if (current.kind !== "split") {
        w.layout.set(id, bufferPath(id), defaultSplit());
        return;
      }
      w.layout.set(id, bufferPath(id), { ...current, ratio: DEFAULT_RATIO });
    },
  });

  registerCommand({
    id: "preview.toggleRunScripts",
    label: "Preview: Toggle run scripts",
    description:
      "Kill switch: when off, the document CSP becomes script-src 'none'. Network stays off regardless.",
    scope: "app",
    global: true,
    execute: () =>
      void toggleRunScripts(() => activeWindow()?.preview.requestForceRefresh()),
  });
}
