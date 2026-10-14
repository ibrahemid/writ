import {
  confirmQuitFlush,
  onWindowCloseRequested,
  onWindowFocusChange,
  recordUnsavedNotes,
  type UnsavedNote,
} from "./tauri";
import { collectUnsavedContent, flushAutosave, type SaveResult } from "./autosave";
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
 * so the unsaved-text journal is the only place left, and the next launch
 * reads it whatever kind of exit this turns out to be. Failing here must not
 * hold the exit: the process leaves either way, and a held quit only trades
 * lost text for a hung app.
 *
 * What the flush just reported is not the question asked. A save that failed
 * minutes ago left the queue empty and the bar on screen, so this flush finds
 * nothing to write and comes back ok while that text is still only in the
 * editor; everything outstanding after the last flush is kept, whatever this
 * one returned.
 */
async function keepUnsavedText(flushed: SaveResult, when: string): Promise<void> {
  if (!flushed.ok) {
    logFailure(`a note could not be saved while ${when}`);
  }

  const notes: UnsavedNote[] = collectUnsavedContent();
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

/**
 * How long closing the window waits on the last writes. A write to a share
 * that stopped answering never settles, and a close that waits on it leaves a
 * window nobody can close.
 */
export const CLOSE_FLUSH_DEADLINE_MS = 3000;

const TIMED_OUT = Symbol("timed out");

/** `work`, or [`TIMED_OUT`] once `ms` have passed without it settling. */
async function within<T>(work: Promise<T>, ms: number): Promise<T | typeof TIMED_OUT> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const deadline = new Promise<typeof TIMED_OUT>((resolve) => {
    timer = setTimeout(() => resolve(TIMED_OUT), ms);
  });
  try {
    return await Promise.race([work, deadline]);
  } finally {
    clearTimeout(timer);
  }
}

export async function installCloseFlush(
  extraFlushes: ReadonlyArray<() => Promise<void> | void> = [],
  deadlineMs: number = CLOSE_FLUSH_DEADLINE_MS,
): Promise<() => void> {
  return onWindowCloseRequested(async () => {
    // Quit is not blocked on a failed write: the error listener has already
    // shown the reason, and holding the window open here would trap the user
    // in an app that cannot save. Nor on a write that never answers: past the
    // deadline the text still out is handed to the journal and the window
    // goes.
    const flushed = await within(flushAutosave(), deadlineMs);
    if (flushed === TIMED_OUT) logFailure("a save did not finish before the window closed");
    const outcome = flushed === TIMED_OUT ? { ok: true, failures: [] } : flushed;
    if ((await within(keepUnsavedText(outcome, "closing"), deadlineMs)) === TIMED_OUT) {
      logFailure("unsaved text could not be kept before the window closed");
    }
    for (const flush of extraFlushes) {
      await within(Promise.resolve().then(flush), deadlineMs);
    }
  });
}
