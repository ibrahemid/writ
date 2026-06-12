import { registerCommand } from "../commands/registry";
import { bufferRegistry } from "../stores/global/buffer-registry";
import { windowRegistry } from "../stores/global/window-registry";
import { toggleRunScripts } from "../lib/preview-actions";
import {
  DEFAULT_RATIO,
  defaultSplit,
  nextCycleLayout,
  type LayoutMode,
} from "../lib/preview-layout";

// Preview keymap (lean scope). Detach is cut, so its binding is gone. The
// ADR-009 force-render binding (Cmd+R) is taken by tab.rename today; L2
// ships preview-refresh on F5 and the rename rebind is a follow-up.

function activeWindow() {
  return windowRegistry.getActive();
}

function activeBufferId(): string | null {
  return activeWindow()?.tabs.activeTabId() ?? null;
}

function bufferPath(bufferId: string): string | null {
  const buf = bufferRegistry.activeTabs().find((b) => b.id === bufferId);
  return buf?.source_path ?? null;
}

/** Register the preview keymap + the run-scripts kill switch palette entry. */
export function registerPreviewKeymap(): void {
  registerCommand({
    id: "preview.cycleLayout",
    label: "Preview: Cycle Layout",
    description: "Source → Split → Preview → Source",
    keybinding: "CmdOrCtrl+Shift+V",
    scope: "app",
    global: true,
    execute: () => {
      const w = activeWindow();
      const id = activeBufferId();
      if (!w || !id) return;
      const next = nextCycleLayout(w.layout.get(id));
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
    label: "Preview: Toggle Fullscreen",
    description: "Show the preview pane only / return to split",
    keybinding: "CmdOrCtrl+Shift+R",
    scope: "app",
    global: true,
    execute: () => {
      const w = activeWindow();
      const id = activeBufferId();
      if (!w || !id) return;
      const current = w.layout.get(id);
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
    label: "Preview: Exit Fullscreen",
    description: "Return from fullscreen preview to the prior layout",
    keybinding: "Escape",
    scope: "app",
    global: true,
    execute: () => {
      const w = activeWindow();
      const id = activeBufferId();
      if (!w || !id) return;
      if (w.layout.get(id).kind !== "preview") return;
      w.layout.restorePrevious(id, bufferPath(id));
    },
  });

  registerCommand({
    id: "preview.swapOrientation",
    label: "Preview: Swap Split Orientation",
    description: "Toggle vertical / horizontal split",
    keybinding: "CmdOrCtrl+Shift+\\",
    scope: "app",
    global: true,
    execute: () => {
      const w = activeWindow();
      const id = activeBufferId();
      if (!w || !id) return;
      const current = w.layout.get(id);
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
    label: "Preview: Reset Split Ratio",
    description: "Reset the split divider to 50/50",
    keybinding: "CmdOrCtrl+0",
    scope: "app",
    global: true,
    execute: () => {
      const w = activeWindow();
      const id = activeBufferId();
      if (!w || !id) return;
      const current = w.layout.get(id);
      if (current.kind !== "split") {
        w.layout.set(id, bufferPath(id), defaultSplit());
        return;
      }
      w.layout.set(id, bufferPath(id), { ...current, ratio: DEFAULT_RATIO });
    },
  });

  registerCommand({
    id: "preview.toggleRunScripts",
    label: "Preview: Toggle Run Scripts",
    description:
      "Kill switch: when off, the document CSP becomes script-src 'none'. Network stays off regardless.",
    scope: "app",
    global: true,
    execute: () =>
      void toggleRunScripts(() => activeWindow()?.preview.requestForceRefresh()),
  });
}
