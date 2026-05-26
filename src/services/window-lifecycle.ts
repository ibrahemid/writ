import { onWindowCloseRequested } from "./tauri";
import { flushAutosave } from "./autosave";

export async function installCloseFlush(
  extraFlushes: ReadonlyArray<() => Promise<void> | void> = [],
): Promise<() => void> {
  return onWindowCloseRequested(async () => {
    await flushAutosave();
    for (const flush of extraFlushes) {
      await flush();
    }
  });
}
