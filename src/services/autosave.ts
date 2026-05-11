import { saveBufferContent } from "./tauri";

type AutosaveErrorListener = (bufferId: string, error: unknown) => void;

const timers = new Map<string, ReturnType<typeof setTimeout>>();
const pendingContent = new Map<string, string>();
const errorListeners = new Set<AutosaveErrorListener>();

export function onAutosaveError(listener: AutosaveErrorListener): () => void {
  errorListeners.add(listener);
  return () => {
    errorListeners.delete(listener);
  };
}

export function debouncedSave(bufferId: string, content: string, delayMs: number = 300) {
  const existing = timers.get(bufferId);
  if (existing) clearTimeout(existing);

  pendingContent.set(bufferId, content);

  const timer = setTimeout(() => {
    timers.delete(bufferId);
    void runPendingSave(bufferId);
  }, delayMs);

  timers.set(bufferId, timer);
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
  const content = pendingContent.get(bufferId);
  if (content === undefined) return;
  pendingContent.delete(bufferId);

  try {
    await saveBufferContent(bufferId, content);
  } catch (error) {
    for (const listener of errorListeners) {
      listener(bufferId, error);
    }
  }
}
