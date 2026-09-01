import {
  confirmQuitFlush,
  onWindowCloseRequested,
  onWindowFocusChange,
  recordUnsavedNotes,
  type UnsavedNote,
} from "./tauri";
import { flushAutosave, peekUnsavedContent, type SaveResult } from "./autosave";
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

/**
 * Puts text the last flush could not write where the next launch will find it.
 *
 * The window is about to go away and the file has already refused this text,
 * so the shutdown snapshot is the only place left; the backend folds it over
 * the files it reads and marks the shutdown unclean, which is what makes the
 * next launch offer it back. Failing here must not hold the exit: the process
 * leaves either way, and a held quit only trades lost text for a hung app.
 */
async function keepUnsavedText(flushed: SaveResult, when: string): Promise<void> {
  if (flushed.ok) return;
  logFailure(`a note could not be saved while ${when}`);

  const notes: UnsavedNote[] = [];
  for (const failure of flushed.failures) {
    const content = peekUnsavedContent(failure.bufferId);
    if (content !== undefined) notes.push({ id: failure.bufferId, content });
  }
  if (notes.length === 0) return;

  try {
    await recordUnsavedNotes(notes);
  } catch {
    logFailure(`unsaved text could not be kept while ${when}`);
  }
}

async function flushThenConfirm(): Promise<void> {
  try {
    await keepUnsavedText(await flushAutosave(), "quitting");
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
    await keepUnsavedText(await flushAutosave(), "closing");
    for (const flush of extraFlushes) {
      await flush();
    }
  });
}
