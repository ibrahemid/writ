import { createSignal } from "solid-js";

// Per-window preview state: view mode per buffer, last render timestamp,
// per-buffer crash counter (3-in-60s suspension rule from ADR-009 failure
// modes), per-buffer session-only policy slot (populated in Phase 3 once the
// trust model lands).

export type ViewMode = "source" | "preview";

interface CrashRecord {
  timestamps: number[];
  suspended: boolean;
}

const CRASH_WINDOW_MS = 60_000;
const CRASH_THRESHOLD = 3;

export type PreviewStore = ReturnType<typeof createPreviewStore>;

export function createPreviewStore() {
  const viewModes = new Map<string, ViewMode>();
  const lastRender = new Map<string, number>();
  const crashes = new Map<string, CrashRecord>();
  const [version, setVersion] = createSignal(0);

  function bump() {
    setVersion((v) => v + 1);
  }

  function getViewMode(bufferId: string): ViewMode {
    void version();
    return viewModes.get(bufferId) ?? "source";
  }

  function setViewMode(bufferId: string, mode: ViewMode): void {
    viewModes.set(bufferId, mode);
    bump();
  }

  function noteRender(bufferId: string, atMs: number = Date.now()): void {
    lastRender.set(bufferId, atMs);
  }

  function lastRenderAt(bufferId: string): number | null {
    return lastRender.get(bufferId) ?? null;
  }

  function recordCrash(bufferId: string, atMs: number = Date.now()): { suspended: boolean } {
    const record = crashes.get(bufferId) ?? { timestamps: [], suspended: false };
    record.timestamps = record.timestamps.filter((t) => atMs - t < CRASH_WINDOW_MS);
    record.timestamps.push(atMs);
    if (record.timestamps.length >= CRASH_THRESHOLD) {
      record.suspended = true;
    }
    crashes.set(bufferId, record);
    bump();
    return { suspended: record.suspended };
  }

  function isSuspended(bufferId: string): boolean {
    void version();
    return crashes.get(bufferId)?.suspended ?? false;
  }

  function clearCrashCounter(bufferId: string): void {
    crashes.delete(bufferId);
    bump();
  }

  return {
    getViewMode,
    setViewMode,
    noteRender,
    lastRenderAt,
    recordCrash,
    isSuspended,
    clearCrashCounter,
  };
}
