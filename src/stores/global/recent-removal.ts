import { bufferRegistry } from "./buffer-registry";
import { showToast } from "./notifications";
import { logFailure } from "../../lib/log";

/**
 * Takes rows out of Recent with an Undo on screen. The rows are deleted when
 * the toast goes, and shown again if Undo is taken. The files stay where they
 * are either way: Recent lists closed tabs, not files.
 */
export function removeFromRecent(ids: readonly string[], text: string): void {
  if (ids.length === 0) return;
  bufferRegistry.hideHistory(ids);
  showToast({
    text,
    type: "success",
    action: { label: "Undo", run: () => bufferRegistry.unhideHistory(ids) },
    onClose: (undone) => {
      if (undone) return;
      bufferRegistry.dropHiddenHistory(ids).catch(() => {
        logFailure("rows could not be removed from Recent");
        showToast("Couldn't remove the files from Recent. They are listed again.", "error");
      });
    },
  });
}

/** [`removeFromRecent`] for one row, named in the toast. */
export function removeOneFromRecent(id: string): void {
  const row = bufferRegistry.buffers().find((b) => b.id === id);
  removeFromRecent([id], row ? `Removed "${row.title}" from Recent` : "Removed from Recent");
}
