import { registerCommand } from "../commands/registry";
import { bufferRegistry } from "../stores/global/buffer-registry";
import { windowRegistry } from "../stores/global/window-registry";
import { contentTypeForBuffer } from "../lib/content-type";
import { previewScriptsStore } from "../stores/global/preview-scripts";
import { showToast } from "../stores/global/notifications";
import {
  DEFAULT_RATIO,
  defaultSplit,
  nextCycleLayout,
  type LayoutMode,
} from "../lib/preview-layout";

// Preview keymap (lean scope). Detach is cut, so its binding is gone; a
// fresh render answers F5.

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

/** The active file's preview type, or null when nothing previewable is open. */
function activeContentType(): string | null {
  const id = activeBufferId();
  return id ? contentTypeOf(id) : null;
}

// What the palette offers, by the active file's type. Availability gates the
// palette only: a chord still reaches its command, which does nothing where
// it has nothing to act on, and Escape still falls through to the editor.

/** A file with any preview: Markdown's Inline view, or a split. */
function previewable(): boolean {
  return activeContentType() !== null;
}

/** A file with a preview pane beside or instead of the text. */
function splits(): boolean {
  const type = activeContentType();
  return type !== null && type !== "markdown";
}

/** A document whose own scripts the preview can run. */
function runsScripts(): boolean {
  const type = activeContentType();
  return type === "html" || type === "markdown";
}

/** Register the preview keymap and the per-file scripts toggle. */
export function registerPreviewKeymap(): void {
  registerCommand({
    id: "preview.cycleLayout",
    // A Markdown file has no preview pane: the chord switches its two views,
    // and the palette row says so while one is active.
    get label() {
      return activeContentType() === "markdown"
        ? "Switch between Inline and Source"
        : "Preview: Cycle layout";
    },
    get description() {
      return activeContentType() === "markdown"
        ? "Show the Markdown formatted in place, or as plain text"
        : "Switch between the text, the text beside its preview, and the preview";
    },
    keywords: ["preview", "layout", "markdown", "inline", "source", "split"],
    keybinding: "CmdOrCtrl+Shift+V",
    scope: "app",
    global: true,
    isAvailable: previewable,
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
    isAvailable: splits,
    execute: () => activeWindow()?.preview.requestForceRefresh(),
  });

  registerCommand({
    id: "preview.toggleFullscreen",
    label: "Preview: Toggle fullscreen",
    description: "Show the preview pane only / return to split",
    keybinding: "CmdOrCtrl+Shift+R",
    scope: "app",
    global: true,
    isAvailable: splits,
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
    isAvailable: () => {
      const w = activeWindow();
      const id = activeBufferId();
      if (!w || !id || !hasSplitLayouts(id)) return false;
      return w.layout.get(id, contentTypeOf(id)).kind === "preview";
    },
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
    isAvailable: splits,
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
    isAvailable: splits,
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
    label: "Preview: Toggle scripts for this file",
    description:
      "Run or stop this file's own scripts in the preview, remembered for the file. Turn on only for a file you trust.",
    scope: "app",
    global: true,
    isAvailable: runsScripts,
    execute: () => {
      const w = activeWindow();
      const id = activeBufferId();
      if (!w || !id) return;
      void previewScriptsStore.toggle(id).then(
        (enabled) => {
          w.preview.requestForceRefresh();
          showToast(
            enabled ? "Scripts on for this file" : "Scripts off for this file",
            "info",
          );
        },
        () => showToast("Could not change scripts for this file", "error"),
      );
    },
  });
}
