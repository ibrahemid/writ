import { createSignal, type Accessor } from "solid-js";
import { onEvent, type UnlistenFn } from "../../services/events";
import { noteBacklinks } from "../../services/tauri";
import type { Backlink } from "../../services/tauri";

// Singleton state — Writ is single-window. The cache is keyed by note path and
// holds only the notes something has asked about, which is the open one and
// whatever a surface is still showing.

export type { Backlink };

/** Shown when the index cannot be read. The list keeps whatever it last had. */
const READ_FAILED_MESSAGE = "Could not read the notes that link here.";

interface Entry {
  rows: Accessor<Backlink[]>;
  setRows: (rows: Backlink[]) => void;
  error: Accessor<string | null>;
  setError: (message: string | null) => void;
}

function createBacklinksStore() {
  const entries = new Map<string, Entry>();
  let subscription: Promise<UnlistenFn> | null = null;

  function entry(path: string): Entry {
    const held = entries.get(path);
    if (held) return held;
    const [rows, setRows] = createSignal<Backlink[]>([]);
    const [error, setError] = createSignal<string | null>(null);
    const created: Entry = { rows, setRows, error, setError };
    entries.set(path, created);
    return created;
  }

  // One listener for the whole cache, opened the first time something asks for
  // a list rather than at launch, so a session that never opens a backlink
  // list never subscribes.
  function subscribe(): Promise<UnlistenFn> {
    subscription ??= onEvent("notes:changed", () => {
      void refreshAll();
    });
    return subscription;
  }

  /**
   * The notes linking to `path`. A note nothing links to reads as `[]` — an
   * empty list, never a placeholder row — so a surface that maps over it
   * renders nothing.
   *
   * The first call starts a read; every later one is served from the cache
   * until a note changes on disk.
   */
  function backlinksFor(path: string): Accessor<Backlink[]> {
    const held = entries.has(path);
    const found = entry(path);
    if (!held) {
      void subscribe();
      void refresh(path);
    }
    return found.rows;
  }

  /** Why the last read of `path` failed, or `null` when it did not. */
  function errorFor(path: string): Accessor<string | null> {
    return entry(path).error;
  }

  /**
   * Reads the list for `path` again.
   *
   * A failed read leaves the rows where they were: a list that empties itself
   * because one call failed reads as "nothing links here", which is the one
   * thing it must not say by accident.
   */
  async function refresh(path: string): Promise<void> {
    const found = entry(path);
    try {
      const rows = await noteBacklinks(path);
      found.setRows(rows);
      found.setError(null);
    } catch {
      found.setError(READ_FAILED_MESSAGE);
    }
  }

  /**
   * Reads every cached list again.
   *
   * A note changing can add or remove links anywhere, and the event names the
   * note that changed rather than the notes whose lists it moved, so every
   * cached path is re-read.
   */
  async function refreshAll(): Promise<void> {
    await Promise.all([...entries.keys()].map((path) => refresh(path)));
  }

  /** Forgets `path`, for a surface that has stopped showing it. */
  function release(path: string): void {
    entries.delete(path);
  }

  /** Drops the cache and the listener. */
  async function reset(): Promise<void> {
    entries.clear();
    const held = subscription;
    subscription = null;
    if (held) (await held)();
  }

  return { backlinksFor, errorFor, refresh, refreshAll, release, reset };
}

export const backlinksStore = createBacklinksStore();
