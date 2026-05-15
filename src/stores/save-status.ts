import { createSignal, createRoot } from "solid-js";
import { onAutosaveError, onAutosaveSuccess } from "../services/autosave";

export type SaveStatus = "idle" | "saved" | "failed";

const SAVED_VISIBLE_MS = 1200;

function createSaveStatusStore() {
  const [status, setStatus] = createSignal<SaveStatus>("idle");
  let clearTimer: ReturnType<typeof setTimeout> | undefined;

  onAutosaveSuccess(() => {
    if (clearTimer) clearTimeout(clearTimer);
    setStatus("saved");
    clearTimer = setTimeout(() => {
      clearTimer = undefined;
      setStatus("idle");
    }, SAVED_VISIBLE_MS);
  });

  onAutosaveError(() => {
    if (clearTimer) {
      clearTimeout(clearTimer);
      clearTimer = undefined;
    }
    setStatus("failed");
  });

  return { status };
}

export const saveStatusStore = createRoot(createSaveStatusStore);
