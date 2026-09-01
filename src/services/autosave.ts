import { isRetryableSaveError } from "../lib/save-error";
import { logFailure } from "../lib/log";
import { recordUnsavedNotes, saveBufferContent } from "./tauri";

type AutosaveErrorListener = (bufferId: string, error: unknown) => void;
// `diskHash` is the digest of what the note's file holds now, or null when the
// note had nothing in it and no file yet to write it to.
type AutosaveSuccessListener = (bufferId: string, diskHash: string | null) => void;
type AutosaveStartListener = (bufferId: string) => void;

// Content may be a string or a lazy getter. A getter is materialized only when
// the save actually runs (timer fire or flush), so a large-buffer edit burst
// never forces a full `doc.toString()` on every keystroke just to feed the
// debounce, and flush stays correct because the getter reads the live document
// at flush time rather than a value captured keystrokes earlier (ADR-020).
export type ContentSource = string | (() => string);

export interface SaveFailure {
  bufferId: string;
  error: unknown;
}

// A save reports its outcome instead of rejecting: every caller awaits it on a
// path (tab close, window close, search) that must keep running when a write
// fails, and the one thing they all need is whether the text reached disk.
export interface SaveResult {
  ok: boolean;
  failures: SaveFailure[];
}

const SAVE_OK: SaveResult = { ok: true, failures: [] };

function saveFailed(bufferId: string, error: unknown): SaveResult {
  return { ok: false, failures: [{ bufferId, error }] };
}

function mergeResults(results: SaveResult[]): SaveResult {
  const failures = results.flatMap((r) => r.failures);
  return { ok: failures.length === 0, failures };
}

// Queued text carries the generation it was queued at. A failed write may put
// its text back only when no newer text has arrived for that buffer since; an
// empty queue is not proof of that, because a second write for the same buffer
// empties it too.
interface QueuedContent {
  source: ContentSource;
  generation: number;
}

interface InFlightWrite {
  promise: Promise<SaveResult>;
  generation: number;
}

// A burst of sub-second pauses (paste, dictation, a macro) fires the idle
// debounce over and over, and every fire is a whole-note rewrite. The cap
// bounds that to one write per note per second. Only the scheduled path is
// capped: a flush and an explicit save are the deterministic "it is on disk"
// paths, and deferring those would lose text the process is about to stop
// being able to write.
const MIN_WRITE_INTERVAL_MS = 1000;

const timers = new Map<string, ReturnType<typeof setTimeout>>();
const lastWriteAt = new Map<string, number>();
const pendingContent = new Map<string, QueuedContent>();
const generations = new Map<string, number>();
const inFlight = new Map<string, InFlightWrite>();
const errorListeners = new Set<AutosaveErrorListener>();
const successListeners = new Set<AutosaveSuccessListener>();
const startListeners = new Set<AutosaveStartListener>();
// The text of the last write that failed, per note, so the quit path can keep
// it when the file could not. Kept beside the queue rather than in it because
// a guard-stopped write deliberately leaves the queue empty (writing the same
// text again is stopped the same way), and that text still has to reach the
// recovery snapshot.
const lastFailedContent = new Map<string, string>();

export function onAutosaveError(listener: AutosaveErrorListener): () => void {
  errorListeners.add(listener);
  return () => {
    errorListeners.delete(listener);
  };
}

export function onAutosaveSuccess(listener: AutosaveSuccessListener): () => void {
  successListeners.add(listener);
  return () => {
    successListeners.delete(listener);
  };
}

/** Fires as a write leaves for the backend, before its outcome is known. */
export function onAutosaveStart(listener: AutosaveStartListener): () => void {
  startListeners.add(listener);
  return () => {
    startListeners.delete(listener);
  };
}

/**
 * The newest text for `bufferId` that is not known to be on disk: whatever is
 * queued, else the text of the write that failed.
 *
 * `undefined` when the note has nothing outstanding. Materializes a queued
 * getter, so a caller reads the live document at the moment it asks.
 */
export function peekUnsavedContent(bufferId: string): string | undefined {
  const queued = pendingContent.get(bufferId);
  if (queued !== undefined) {
    try {
      return typeof queued.source === "function" ? queued.source() : queued.source;
    } catch {
      // The live document is gone. Fall through to the failed text, which is
      // a plain string and outlives the view.
    }
  }
  return lastFailedContent.get(bufferId);
}

/**
 * Every note holding text that is not known to be on disk, with that text.
 *
 * The queue is not enough on its own: a write stopped by the guard empties the
 * queue on purpose, because writing the same text again is stopped the same
 * way, so the note whose failure is still on screen is exactly the one a
 * queue-only walk would miss.
 */
export function collectUnsavedContent(): Array<{ id: string; content: string }> {
  const ids = new Set([...pendingContent.keys(), ...lastFailedContent.keys()]);
  const notes: Array<{ id: string; content: string }> = [];
  for (const id of ids) {
    const content = peekUnsavedContent(id);
    if (content !== undefined) notes.push({ id, content });
  }
  return notes;
}

function bumpGeneration(bufferId: string): number {
  const next = (generations.get(bufferId) ?? 0) + 1;
  generations.set(bufferId, next);
  return next;
}

function clearTimer(bufferId: string) {
  const existing = timers.get(bufferId);
  if (existing) {
    clearTimeout(existing);
    timers.delete(bufferId);
  }
}

function queueContent(bufferId: string, content: ContentSource) {
  pendingContent.set(bufferId, { source: content, generation: bumpGeneration(bufferId) });
}

export function debouncedSave(bufferId: string, content: ContentSource, delayMs: number = 1000) {
  clearTimer(bufferId);
  queueContent(bufferId, content);

  armTimer(bufferId, delayMs);
}

function armTimer(bufferId: string, delayMs: number) {
  const timer = setTimeout(() => {
    timers.delete(bufferId);
    runScheduledSave(bufferId);
  }, delayMs);

  timers.set(bufferId, timer);
}

// A write that would land inside the cap is re-armed for the remainder of the
// window, never dropped: the text is still the newest the user typed, and a
// dropped write is the one failure autosave may not have.
function runScheduledSave(bufferId: string) {
  const last = lastWriteAt.get(bufferId);
  const waited = last === undefined ? MIN_WRITE_INTERVAL_MS : Date.now() - last;
  if (waited < MIN_WRITE_INTERVAL_MS) {
    armTimer(bufferId, MIN_WRITE_INTERVAL_MS - waited);
    return;
  }
  void runPendingSave(bufferId);
}

export function hasPendingAutosave(bufferId: string): boolean {
  return pendingContent.has(bufferId) || timers.has(bufferId) || inFlight.has(bufferId);
}

/**
 * Hands a closing note's unwritten text to the recovery snapshot, then drops
 * the record of it.
 *
 * A save the guard refused does not go back on the queue — writing the same
 * text into the same refusal is stopped the same way — so closing that tab
 * finds nothing to flush and closes without a word, while the text is still
 * only in this module. Left there it would be snapshotted at the next quit,
 * long after the tab went, and restored as a note the person had closed.
 * Handed over here it survives as itself.
 *
 * The record is kept when the handover fails: text nobody can read again is a
 * worse outcome than a note restored twice.
 */
export async function keepUnsavedForRecovery(bufferId: string): Promise<void> {
  const content = peekUnsavedContent(bufferId);
  if (content !== undefined) {
    try {
      await recordUnsavedNotes([{ id: bufferId, content }]);
    } catch {
      logFailure("the text of a closed note could not be kept");
      return;
    }
  }
  cancelAutosave(bufferId);
}

export function cancelAutosave(bufferId: string) {
  clearTimer(bufferId);
  pendingContent.delete(bufferId);
  lastWriteAt.delete(bufferId);
  lastFailedContent.delete(bufferId);
  // Retire the current generation so a write already in flight cannot put the
  // discarded text back on the queue when it fails.
  bumpGeneration(bufferId);
}

// Drops every scheduled save, queued edit and write-rate record at once.
// Autosave is a process-wide singleton (Writ is single-window), so a caller
// that drives the module in isolation — a test case, or a teardown — has no
// other way back to the start state; `cancelAutosave` is the per-note sibling.
export function resetAutosave() {
  for (const timer of timers.values()) clearTimeout(timer);
  timers.clear();
  pendingContent.clear();
  lastWriteAt.clear();
  generations.clear();
  inFlight.clear();
  lastFailedContent.clear();
}

export async function flushAutosave(bufferId?: string): Promise<SaveResult> {
  if (bufferId !== undefined) {
    clearTimer(bufferId);
    return runPendingSave(bufferId);
  }

  const ids = new Set<string>([
    ...timers.keys(),
    ...pendingContent.keys(),
    ...inFlight.keys(),
  ]);
  for (const timer of timers.values()) clearTimeout(timer);
  timers.clear();
  const results = await Promise.all(Array.from(ids, (id) => runPendingSave(id)));
  return mergeResults(results);
}

// Writes the buffer now whether or not an edit is pending: an explicit save is
// a deterministic "it is on disk" action, so it must not fall through to a
// no-op the way flushing an empty queue does. Reports through the same success
// and error listeners as an autosave.
export async function saveNow(bufferId: string, content: ContentSource): Promise<SaveResult> {
  clearTimer(bufferId);
  queueContent(bufferId, content);
  return runPendingSave(bufferId);
}

// One write per buffer at a time. Concurrent writes for one buffer race on the
// file and on the retry queue, and a caller that returns while a write is still
// outstanding lets the close path drop text that never reached disk. A caller
// arriving during a write waits for it, folds its outcome in, then writes
// whatever is queued behind it.
async function runPendingSave(bufferId: string): Promise<SaveResult> {
  const current = inFlight.get(bufferId);
  if (current === undefined) return startWrite(bufferId);

  const outcome = await current.promise;
  if (!pendingContent.has(bufferId) && !inFlight.has(bufferId)) return outcome;
  return mergeResults([outcome, await runPendingSave(bufferId)]);
}

async function startWrite(bufferId: string): Promise<SaveResult> {
  const queued = pendingContent.get(bufferId);
  if (queued === undefined) return SAVE_OK;
  pendingContent.delete(bufferId);

  const promise = writeQueued(bufferId, queued);
  inFlight.set(bufferId, { promise, generation: queued.generation });
  try {
    return await promise;
  } finally {
    if (inFlight.get(bufferId)?.promise === promise) inFlight.delete(bufferId);
  }
}

async function writeQueued(bufferId: string, queued: QueuedContent): Promise<SaveResult> {
  let content: string;
  try {
    content = typeof queued.source === "function" ? queued.source() : queued.source;
  } catch (error) {
    // The live document is gone (e.g. the view was torn down between schedule
    // and fire). Nothing to save; surface it like any other autosave failure.
    for (const listener of errorListeners) {
      listener(bufferId, error);
    }
    return saveFailed(bufferId, error);
  }

  lastWriteAt.set(bufferId, Date.now());
  for (const listener of startListeners) {
    listener(bufferId);
  }
  try {
    const diskHash = await saveBufferContent(bufferId, content);
    lastFailedContent.delete(bufferId);
    for (const listener of successListeners) {
      listener(bufferId, diskHash);
    }
    return SAVE_OK;
  } catch (error) {
    // A failed write must not consume the text. It goes back on the queue as a
    // plain string (the getter's document may be gone by the next attempt) so
    // the next scheduled save or flush writes it again, but only while it is
    // still the newest text for this buffer.
    //
    // A write the guard stopped is the exception. Identical text is stopped
    // the same way, and a stopped save lands another dated copy beside the
    // note each time, so this text leaves the queue and the next keystroke —
    // which queues a new generation — is what writes again.
    if (generations.get(bufferId) === queued.generation && isRetryableSaveError(error)) {
      pendingContent.set(bufferId, { source: content, generation: queued.generation });
    }
    lastFailedContent.set(bufferId, content);
    for (const listener of errorListeners) {
      listener(bufferId, error);
    }
    return saveFailed(bufferId, error);
  }
}
