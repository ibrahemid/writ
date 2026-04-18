import { saveBufferContent } from "./tauri";

type AutosaveErrorListener = (bufferId: string, error: unknown) => void;

const timers = new Map<string, ReturnType<typeof setTimeout>>();
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

  const timer = setTimeout(async () => {
    timers.delete(bufferId);
    try {
      await saveBufferContent(bufferId, content);
    } catch (error) {
      for (const listener of errorListeners) {
        listener(bufferId, error);
      }
    }
  }, delayMs);

  timers.set(bufferId, timer);
}

export function cancelAutosave(bufferId: string) {
  const existing = timers.get(bufferId);
  if (existing) {
    clearTimeout(existing);
    timers.delete(bufferId);
  }
}
