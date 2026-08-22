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

const timers = new Map<string, ReturnType<typeof setTimeout>>();
const pendingContent = new Map<string, ContentSource>();
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

export function debouncedSave(bufferId: string, content: ContentSource, delayMs: number = 300) {
  const existing = timers.get(bufferId);
  if (existing) clearTimeout(existing);

  pendingContent.set(bufferId, content);

  const timer = setTimeout(() => {
    timers.delete(bufferId);
    void runPendingSave(bufferId);
  }, delayMs);

  timers.set(bufferId, timer);
}

export function hasPendingAutosave(bufferId: string): boolean {
  return pendingContent.has(bufferId) || timers.has(bufferId);
}

export function cancelAutosave(bufferId: string) {
  const existing = timers.get(bufferId);
  if (existing) {
    clearTimeout(existing);
    timers.delete(bufferId);
  }
  pendingContent.delete(bufferId);
}

export async function flushAutosave(bufferId?: string): Promise<SaveResult> {
  if (bufferId !== undefined) {
    const timer = timers.get(bufferId);
    if (timer) {
      clearTimeout(timer);
      timers.delete(bufferId);
    }
    if (pendingContent.has(bufferId)) {
      return runPendingSave(bufferId);
    }
    return SAVE_OK;
  }

  const ids = new Set<string>([...timers.keys(), ...pendingContent.keys()]);
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
  const timer = timers.get(bufferId);
  if (timer) {
    clearTimeout(timer);
    timers.delete(bufferId);
  }
  pendingContent.set(bufferId, content);
  return runPendingSave(bufferId);
}

async function runPendingSave(bufferId: string): Promise<SaveResult> {
  const source = pendingContent.get(bufferId);
  if (source === undefined) return SAVE_OK;
  pendingContent.delete(bufferId);

  let content: string;
  try {
    content = typeof source === "function" ? source() : source;
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
    // plain string (the getter's document may be gone by the retry) so the next
    // scheduled save or flush writes it again. Content that arrived while this
    // write was in flight is newer and wins.
    if (!pendingContent.has(bufferId)) {
      pendingContent.set(bufferId, content);
    }
    for (const listener of errorListeners) {
      listener(bufferId, error);
    }
    return saveFailed(bufferId, error);
  }
}
