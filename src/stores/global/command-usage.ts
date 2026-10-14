import { createRoot } from "solid-js";
import { createStore, produce, reconcile } from "solid-js/store";
import * as api from "../../services/tauri";
import { logFailure } from "../../lib/log";
import type { CommandUsage, CommandUse } from "../../types/command-usage";

// Singleton — app-global, not window-scoped (ADR-009 E3).
// How often each palette command ran, which ranks the palette. Kept in
// writ.db, not in the config (ADR-048): running a command touches no setting
// and no reader of one.

const FLUSH_DEBOUNCE_MS = 750;

function createCommandUsageStore() {
  const [usage, setUsage] = createStore<Record<string, CommandUsage>>({});
  // Runs not yet written. A held chord repeats a command many times a second,
  // so the runs are added up here and written once the burst ends.
  let pending = new Map<string, CommandUse>();
  let flushTimer: ReturnType<typeof setTimeout> | null = null;
  let writeFailureReported = false;

  async function load(): Promise<void> {
    try {
      const stored = await api.getCommandUsage();
      // Runs recorded while the read was on its way are not in its answer.
      for (const queued of pending.values()) {
        const entry = stored[queued.id];
        stored[queued.id] = {
          count: (entry?.count ?? 0) + queued.count,
          last_used_ms: Math.max(entry?.last_used_ms ?? 0, queued.last_used_ms),
        };
      }
      setUsage(reconcile(stored));
    } catch {
      logFailure("command usage could not be read");
    }
  }

  function record(id: string, nowMs: number = Date.now()) {
    const previous = usage[id];
    setUsage(id, { count: (previous?.count ?? 0) + 1, last_used_ms: nowMs });
    const queued = pending.get(id);
    pending.set(id, { id, count: (queued?.count ?? 0) + 1, last_used_ms: nowMs });
    if (flushTimer) clearTimeout(flushTimer);
    flushTimer = setTimeout(() => {
      flushTimer = null;
      void flush();
    }, FLUSH_DEBOUNCE_MS);
  }

  /** Writes the runs recorded since the last write. */
  async function flush(): Promise<void> {
    if (flushTimer) {
      clearTimeout(flushTimer);
      flushTimer = null;
    }
    if (pending.size === 0) return;
    const uses = [...pending.values()];
    pending = new Map();
    try {
      await api.recordCommandUses(uses);
      writeFailureReported = false;
    } catch {
      if (writeFailureReported) return;
      writeFailureReported = true;
      logFailure("command usage could not be written");
    }
  }

  function dropPending() {
    if (flushTimer) {
      clearTimeout(flushTimer);
      flushTimer = null;
    }
    pending = new Map();
  }

  async function clear(): Promise<void> {
    dropPending();
    await api.clearCommandUsage();
    setUsage(reconcile({}));
  }

  /** Forgets the commands that no longer exist. */
  function prune(knownIds: ReadonlySet<string>) {
    setUsage(
      produce((held) => {
        for (const id of Object.keys(held)) {
          if (!knownIds.has(id)) delete held[id];
        }
      }),
    );
    for (const id of [...pending.keys()]) {
      if (!knownIds.has(id)) pending.delete(id);
    }
    api.pruneCommandUsage([...knownIds]).catch(() => {
      logFailure("command usage could not be pruned");
    });
  }

  return {
    /** Every command's usage. Reactive per command. */
    usage: (): Readonly<Record<string, CommandUsage>> => usage,
    load,
    record,
    flush,
    clear,
    prune,
  };
}

export const commandUsageStore = createRoot(createCommandUsageStore);
