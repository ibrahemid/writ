import { windowRegistry } from "../../stores/global/window-registry";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { basename, pathKey } from "../../lib/path";
import { logFailure } from "../../lib/log";
import { showToast } from "../../components/Notifications/Toast";
import type { BufferDocument } from "../../types/buffer";

export type OpenTarget =
  | { kind: "buffer"; id: string }
  | { kind: "history"; id: string }
  | { kind: "file"; path: string };

export interface BufferTarget {
  doc: BufferDocument;
  kind: "buffer" | "history";
}

// Open tabs first, then history. The order is the dedupe precedence: a buffer
// carries unsaved state, so it wins over the same path on disk.
export function bufferTargets(): BufferTarget[] {
  const targets: BufferTarget[] = bufferRegistry
    .activeTabs()
    .map((doc) => ({ doc, kind: "buffer" as const }));
  const seen = new Set(targets.map((t) => t.doc.id));
  for (const doc of bufferRegistry.historyList()) {
    if (seen.has(doc.id)) continue;
    seen.add(doc.id);
    targets.push({ doc, kind: "history" });
  }
  return targets;
}

export function bufferPathKeys(targets: readonly BufferTarget[]): Set<string> {
  const keys = new Set<string>();
  for (const target of targets) {
    if (target.doc.source_path) keys.add(pathKey(target.doc.source_path));
  }
  return keys;
}

// Opens a palette result and, when the row carries one, reveals its line. A
// history entry is restored first; a workspace file is opened and revealed on
// the buffer id the open returns.
export function openTarget(target: OpenTarget, line?: number): void {
  const win = windowRegistry.getActive();
  if (!win) return;

  if (target.kind === "buffer") {
    win.tabs.setActiveTabId(target.id);
    if (line !== undefined) win.editor.requestReveal(target.id, line);
    return;
  }

  if (target.kind === "history") {
    win.tabs.restoreFromHistory(target.id).then(
      () => {
        if (line !== undefined) win.editor.requestReveal(target.id, line);
      },
      () => {
        logFailure("a history entry could not be restored");
        showToast("Couldn't reopen that file", "error");
      },
    );
    return;
  }

  win.tabs.openFile(target.path).then(
    (doc) => {
      // A note still downloading opens no buffer, so there is no line to go to.
      if (doc && line !== undefined) win.editor.requestReveal(doc.id, line);
    },
    () => {
      logFailure("a file could not be opened from the palette");
      showToast(`Couldn't open ${basename(target.path)}`, "error");
    },
  );
}
