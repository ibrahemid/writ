import { windowRegistry } from "../stores/global/window-registry";
import { showToast } from "../stores/global/notifications";

/** What Open Recent says when no file has been closed yet. */
export const NOTHING_CLOSED = "Nothing closed yet.";

/**
 * File > Open Recent and the palette's Open recent: the sidebar opens on the
 * Recent list, or, with nothing closed yet, a line says so and nothing else
 * moves (NV-36).
 */
export function openRecent(): void {
  const win = windowRegistry.getActive();
  if (!win) return;
  if (!win.sidebar.showRecent()) showToast(NOTHING_CLOSED, "info");
}
