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
  /** Bumped per read, so a slow earlier read cannot land after a later one. */
  generation: number;
  /** Whether something is showing this list. A released note stops refreshing. */
  held: boolean;
}

function createBacklinksStore() {
  const entries = new Map<string, Entry>();
  let subscription: Promise<UnlistenFn> | null = null;

  function entry(path: string): Entry {
    const held = entries.get(path);
    if (held) return held;
    const [rows, setRows] = createSignal<Backlink[]>([]);
    const [error, setError] = createSignal<string | null>(null);
    const created: Entry = { rows, setRows, error, setError, generation: 0, held: false };
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
   *
   * The same accessor comes back for the life of the store, so a caller that
   * kept one across a [`release`] keeps seeing the list.
   */
  function backlinksFor(path: string): Accessor<Backlink[]> {
    const found = entry(path);
    if (!found.held) {
      found.held = true;
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
   *
   * A burst of changes starts overlapping reads of the same note. Only the
   * newest one may write, in success and in failure alike, so the list cannot
   * settle on an older answer or wear an error a newer read cleared.
   */
  async function refresh(path: string): Promise<void> {
    const found = entry(path);
    const ticket = ++found.generation;
    try {
      const rows = await noteBacklinks(path);
      if (ticket !== found.generation) return;
      found.setRows(rows);
      found.setError(null);
    } catch {
      if (ticket !== found.generation) return;
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
    const held = [...entries.entries()].filter(([, found]) => found.held);
    await Promise.all(held.map(([path]) => refresh(path)));
  }

  /**
   * Stops following `path`, for a surface that has stopped showing it. The
   * list empties and the next ask reads it again; a caller still holding the
   * accessor is not left on a signal nothing writes to.
   */
  function release(path: string): void {
    const found = entries.get(path);
    if (!found) return;
    found.held = false;
    found.generation += 1;
    found.setRows([]);
    found.setError(null);
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
