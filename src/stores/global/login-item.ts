import { createRoot, createSignal } from "solid-js";
import * as api from "../../services/tauri";
import { logFailure } from "../../lib/log";
import { showToast } from "./notifications";

// Singleton state — Writ is single-window. Whether the OS starts Writ at
// login, as the OS has it.

export function createLoginItemStore() {
  const [enabled, setEnabled] = createSignal(false);
  const [busy, setBusy] = createSignal(false);

  async function load(): Promise<void> {
    try {
      setEnabled((await api.launchAtLoginStatus()).enabled);
    } catch {
      logFailure("whether Writ opens at login could not be read");
    }
  }

  /** Asks the OS to start Writ at login, or to stop. A refusal leaves the
   * switch where it was and says so. */
  async function set(on: boolean): Promise<void> {
    if (busy()) return;
    setBusy(true);
    try {
      setEnabled((await api.setLaunchAtLogin(on)).enabled);
    } catch {
      logFailure("open at login could not be changed");
      showToast(on ? "Couldn't add Writ to your login items" : "Couldn't remove Writ from your login items", "error");
    } finally {
      setBusy(false);
    }
  }

  return { enabled, busy, load, set };
}

export const loginItemStore = createRoot(createLoginItemStore);
