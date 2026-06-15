import { saveBufferContent } from "./tauri";

type AutosaveErrorListener = (bufferId: string, error: unknown) => void;
type AutosaveSuccessListener = (bufferId: string) => void;

// Content may be a string or a lazy getter. A getter is materialized only when
// the save actually runs (timer fire or flush), so a large-buffer edit burst
// never forces a full `doc.toString()` on every keystroke just to feed the
// debounce, and flush stays correct because the getter reads the live document
// at flush time rather than a value captured keystrokes earlier (ADR-020).
type ContentSource = string | (() => string);

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

export async function flushAutosave(bufferId?: string): Promise<void> {
  if (bufferId !== undefined) {
    const timer = timers.get(bufferId);
    if (timer) {
      clearTimeout(timer);
      timers.delete(bufferId);
    }
    if (pendingContent.has(bufferId)) {
      await runPendingSave(bufferId);
    }
    return;
  }

  const ids = new Set<string>([...timers.keys(), ...pendingContent.keys()]);
  for (const timer of timers.values()) clearTimeout(timer);
  timers.clear();
  await Promise.all(Array.from(ids, (id) => runPendingSave(id)));
}

async function runPendingSave(bufferId: string): Promise<void> {
  const source = pendingContent.get(bufferId);
  if (source === undefined) return;
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
    return;
  }

  try {
    await saveBufferContent(bufferId, content);
    for (const listener of successListeners) {
      listener(bufferId);
    }
  } catch (error) {
    for (const listener of errorListeners) {
      listener(bufferId, error);
    }
  }
}
