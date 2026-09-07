import { createRoot, createSignal } from "solid-js";
import * as api from "../../services/tauri";
import { onEvent, type UnlistenFn } from "../../services/events";
import { logFailure } from "../../lib/log";

// Singleton state — Writ is single-window. One chord shows and hides the
// window, and one answer says whether this machine let Writ have it.

export function createHotkeyStore() {
  const [chord, setChord] = createSignal("");
  const [isTaken, setIsTaken] = createSignal(false);

  function apply(status: api.GlobalHotkeyStatus): void {
    setChord(status.chord);
    setIsTaken(!status.registered);
  }

  // Read once at startup as well as listened for: the answer is settled before
  // the window exists, so the event that carried it has already been and gone.
  async function load(): Promise<void> {
    try {
      apply(await api.globalHotkeyStatus());
    } catch {
      logFailure("the global shortcut's state could not be read");
    }
  }

  async function subscribe(): Promise<UnlistenFn> {
    return onEvent("hotkey:status", apply);
  }

  /** Asks the OS for `next`, and answers whether it was given. */
  async function rebind(next: string): Promise<boolean> {
    const status = await api.setGlobalHotkey(next);
    apply(status);
    return status.registered;
  }

  return { chord, isTaken, load, subscribe, rebind };
}

export const hotkeyStore = createRoot(createHotkeyStore);
