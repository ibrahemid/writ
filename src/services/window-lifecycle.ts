import { onWindowCloseRequested } from "./tauri";
import { flushAutosave } from "./autosave";
import { logFailure } from "../lib/log";

export async function installCloseFlush(
  extraFlushes: ReadonlyArray<() => Promise<void> | void> = [],
): Promise<() => void> {
  return onWindowCloseRequested(async () => {
    // Quit is not blocked on a failed write: the error listener has already
    // shown the reason, and holding the window open here would trap the user
    // in an app that cannot save.
    const flushed = await flushAutosave();
    if (!flushed.ok) {
      logFailure("a buffer could not be saved while closing");
    }
    for (const flush of extraFlushes) {
      await flush();
    }
  });
}
