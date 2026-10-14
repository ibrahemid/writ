import { createRoot, createSignal } from "solid-js";
import * as api from "../../services/tauri";
import { onEvent, type UnlistenFn } from "../../services/events";
import { logFailure } from "../../lib/log";

/** The chord Writ ships with for showing and hiding the window. */
export const SHIPPED_WINDOW_CHORD = "CmdOrCtrl+Shift+Space";

/** The chord Writ ships with for the quick capture note (ADR-055). */
export const SHIPPED_CAPTURE_CHORD = "CmdOrCtrl+Shift+Alt+Space";

/** How one chord that works from any app is read, listened for and changed.
 * Functions rather than the service exports themselves, so a test that mocks
 * the services with only the window's calls can still load this module. */
interface GlobalChordCalls {
  status: () => Promise<api.GlobalHotkeyStatus>;
  rebind: (chord: string) => Promise<api.HotkeyRebind>;
  subscribe: (apply: (status: api.GlobalHotkeyStatus) => void) => Promise<UnlistenFn>;
  /** What a failed read is logged as. */
  unread: string;
}

// Singleton state — Writ is single-window. Each chord that works from any app
// has one answer saying whether this machine let Writ have it.

function createGlobalChordStore(calls: GlobalChordCalls) {
  const [chord, setChord] = createSignal("");
  const [isTaken, setIsTaken] = createSignal(false);
  const [isUnsupported, setIsUnsupported] = createSignal(false);

  function apply(status: api.GlobalHotkeyStatus): void {
    setChord(status.chord);
    setIsUnsupported(status.unsupported === true);
    // An empty chord is one turned off, which nothing else can be holding.
    setIsTaken(status.chord !== "" && !status.registered && status.unsupported !== true);
  }

  // Read once at startup as well as listened for: the answer is settled before
  // the window exists, so the event that carried it has already been and gone.
  async function load(): Promise<void> {
    try {
      apply(await calls.status());
    } catch {
      logFailure(calls.unread);
    }
  }

  async function subscribe(): Promise<UnlistenFn> {
    return calls.subscribe(apply);
  }

  /** Asks the OS for `next`, and answers whether it was given. A refusal
   * keeps the chord there was, which `chord()` then still names. */
  async function rebind(next: string): Promise<boolean> {
    const answer = await calls.rebind(next);
    apply(answer.status);
    return !answer.refused && (answer.status.registered || next === "");
  }

  return { chord, isTaken, isUnsupported, load, subscribe, rebind };
}

export function createHotkeyStore() {
  return createGlobalChordStore({
    status: () => api.globalHotkeyStatus(),
    rebind: (chord) => api.setGlobalHotkey(chord),
    subscribe: (apply) => onEvent("hotkey:status", apply),
    unread: "the global shortcut's state could not be read",
  });
}

export function createCaptureHotkeyStore() {
  return createGlobalChordStore({
    status: () => api.captureHotkeyStatus(),
    rebind: (chord) => api.setCaptureHotkey(chord),
    subscribe: (apply) => onEvent("hotkey:capture-status", apply),
    unread: "the capture shortcut's state could not be read",
  });
}

export const hotkeyStore = createRoot(createHotkeyStore);
export const captureHotkeyStore = createRoot(createCaptureHotkeyStore);
