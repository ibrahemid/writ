import { onWindowCloseRequested } from "./tauri";
import { flushAutosave } from "./autosave";

export async function installCloseFlush(): Promise<() => void> {
  return onWindowCloseRequested(() => flushAutosave());
}
