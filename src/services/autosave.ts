import { saveBufferContent } from "./tauri";

let timers = new Map<string, ReturnType<typeof setTimeout>>();

export function debouncedSave(bufferId: string, content: string, delayMs: number = 300) {
  const existing = timers.get(bufferId);
  if (existing) clearTimeout(existing);

  const timer = setTimeout(async () => {
    timers.delete(bufferId);
    try {
      await saveBufferContent(bufferId, content);
    } catch (e) {
      console.error("autosave failed:", e);
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
