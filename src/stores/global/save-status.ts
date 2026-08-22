import { createSignal, createRoot } from "solid-js";
import { onAutosaveError, onAutosaveSuccess } from "../../services/autosave";
import { formatSaveError } from "../../lib/save-error";

// Singleton — app-global, not window-scoped (ADR-009 E3).
// Autosave runs once for the whole app; this status mirrors that single pipeline.

export type SaveStatus = "idle" | "saved" | "failed";

const SAVED_VISIBLE_MS = 1200;

function createSaveStatusStore() {
  const [status, setStatus] = createSignal<SaveStatus>("idle");
  const [lastError, setLastError] = createSignal<string | null>(null);
  let clearTimer: ReturnType<typeof setTimeout> | undefined;

  onAutosaveSuccess(() => {
    if (clearTimer) clearTimeout(clearTimer);
    setLastError(null);
    setStatus("saved");
    clearTimer = setTimeout(() => {
      clearTimer = undefined;
      setStatus("idle");
    }, SAVED_VISIBLE_MS);
  });

  onAutosaveError((_bufferId, error) => {
    if (clearTimer) {
      clearTimeout(clearTimer);
      clearTimer = undefined;
    }
    setLastError(formatSaveError(error));
    setStatus("failed");
  });

  return { status, lastError };
}

export const saveStatusStore = createRoot(createSaveStatusStore);
