import { confirmQuitFlush, onWindowCloseRequested, onWindowFocusChange } from "./tauri";
import { flushAutosave } from "./autosave";
import { onEvent, type UnlistenFn } from "./events";
import { logFailure } from "../lib/log";

// Every way a note stops being looked at rides one mechanism. Hiding the
// window drops focus, so the global toggle and the Dock arrive here alongside
// clicking away, and none of them needs a path of its own.
//
// Quit is the exception, because nothing drops focus on the way out: the
// shutdown path asks for the flush and waits for the answer, so the answer has
// to go back even when a write failed. Withholding it only holds the quit for
// the timeout and then exits anyway.
export async function startWindowLifecycle(): Promise<UnlistenFn[]> {
  const unlistenFocus = await onWindowFocusChange((focused) => {
    if (!focused) void flushAutosave();
  });

  const unlistenQuit = await onEvent("quit:flush", () => {
    void flushThenConfirm();
  });

  return [unlistenFocus, unlistenQuit];
}

async function flushThenConfirm(): Promise<void> {
  try {
    const flushed = await flushAutosave();
    if (!flushed.ok) {
      logFailure("a note could not be saved while quitting");
    }
  } finally {
    await confirmQuitFlush();
  }
}

export async function installCloseFlush(
  extraFlushes: ReadonlyArray<() => Promise<void> | void> = [],
): Promise<() => void> {
  return onWindowCloseRequested(async () => {
    // Quit is not blocked on a failed write: the error listener has already
    // shown the reason, and holding the window open here would trap the user
    // in an app that cannot save.
    const flushed = await flushAutosave();
    if (!flushed.ok) {
      logFailure("a note could not be saved while closing");
    }
    for (const flush of extraFlushes) {
      await flush();
    }
  });
}
