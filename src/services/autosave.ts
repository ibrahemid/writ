import { isRetryableSaveError } from "../lib/save-error";
import { saveBufferContent } from "./tauri";

type AutosaveErrorListener = (bufferId: string, error: unknown) => void;
type AutosaveSuccessListener = (bufferId: string) => void;

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

const timers = new Map<string, ReturnType<typeof setTimeout>>();
const pendingContent = new Map<string, QueuedContent>();
const generations = new Map<string, number>();
const inFlight = new Map<string, InFlightWrite>();
const errorListeners = new Set<AutosaveErrorListener>();
const successListeners = new Set<AutosaveSuccessListener>();

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

  const timer = setTimeout(() => {
    timers.delete(bufferId);
    void runPendingSave(bufferId);
  }, delayMs);

  timers.set(bufferId, timer);
}

export function hasPendingAutosave(bufferId: string): boolean {
  return pendingContent.has(bufferId) || timers.has(bufferId) || inFlight.has(bufferId);
}

export function cancelAutosave(bufferId: string) {
  clearTimer(bufferId);
  pendingContent.delete(bufferId);
  // Retire the current generation so a write already in flight cannot put the
  // discarded text back on the queue when it fails.
  bumpGeneration(bufferId);
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

  try {
    await saveBufferContent(bufferId, content);
    for (const listener of successListeners) {
      listener(bufferId);
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
    for (const listener of errorListeners) {
      listener(bufferId, error);
    }
    return saveFailed(bufferId, error);
  }
}
