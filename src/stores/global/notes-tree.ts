import { createEffect, createRoot, on } from "solid-js";
import { createStore, produce, reconcile } from "solid-js/store";
import * as tauri from "../../services/tauri";
import { onEvent, type UnlistenFn } from "../../services/events";
import { dirname, isSameOrInside } from "../../lib/path";
import { notesStore } from "./notes";
import type { WorkspaceEntry } from "../../types/workspace";

// Singleton state — Writ is single-window. The listing of the notes folder the
// sidebar draws when no other folder is open (NV-01): one array per folder the
// tree has opened, read from disk and kept in step with the folder by the
// three events that say it changed.

const LIST_FAILED = "Could not read this folder.";

function createNotesTreeStore() {
  const [dirs, setDirs] = createStore<Record<string, WorkspaceEntry[] | undefined>>({});
  const [errors, setErrors] = createStore<Record<string, string | undefined>>({});
  const loading = new Map<string, Promise<void>>();
  const owed = new Set<string>();
  let subscriptions: Promise<UnlistenFn>[] | null = null;

  function root(): string | null {
    return notesStore.root();
  }

  function forgetAll(): void {
    setDirs(reconcile({}));
    setErrors(reconcile({}));
    owed.clear();
  }

  // A moved notes folder is another folder: nothing listed under the old one
  // describes it.
  createEffect(on(notesStore.root, forgetAll, { defer: true }));

  // Opened the first time the tree lists anything, so a sidebar showing
  // another folder never subscribes.
  function subscribe(): void {
    subscriptions ??= [
      onEvent("notes:changed", (payload) => {
        if (payload) handleChanged(payload.path, payload.removed);
      }),
      onEvent("notes:indexed", (payload) => {
        if (payload) handleIndexed(payload.paths);
      }),
      onEvent("notes:swept", () => {
        for (const path of new Set([...Object.keys(dirs), ...Object.keys(errors)])) {
          if (asked(path)) void loadDir(path);
        }
      }),
    ];
  }

  /**
   * Lists `path` from disk.
   *
   * Entries are matched to the ones already listed by path, so a folder the
   * person opened in the tree stays open when the folder around it is listed
   * again. A listing asked for while one is running is owed one more listing
   * after it, however many times it was asked. A failed listing keeps what
   * was listed before and says why.
   */
  function loadDir(path: string): Promise<void> {
    subscribe();
    const running = loading.get(path);
    if (running) {
      owed.add(path);
      return running;
    }
    const base = root();
    const run = (async () => {
      try {
        do {
          owed.delete(path);
          try {
            const entries = await tauri.listNotesDir(path);
            if (root() !== base) return;
            setDirs(path, reconcile(entries, { key: "path" }));
            setErrors(path, undefined);
          } catch {
            if (root() !== base) return;
            setErrors(path, LIST_FAILED);
          }
        } while (owed.has(path));
      } finally {
        loading.delete(path);
      }
    })();
    loading.set(path, run);
    return run;
  }

  function entriesFor(path: string): WorkspaceEntry[] | undefined {
    return dirs[path];
  }

  function errorFor(path: string): string | null {
    return errors[path] ?? null;
  }

  /**
   * Whether the tree has asked for `path`: it is listed, or its listing
   * failed. A failed folder is read again on the next change inside it.
   */
  function asked(path: string): boolean {
    return dirs[path] !== undefined || errors[path] !== undefined;
  }

  /**
   * The deepest folder the tree has asked for that holds `path`, `path` itself
   * when it is one. A file created in a folder the tree has never opened
   * shows up in the nearest folder it has: the new folder holding it is
   * news there.
   */
  function listedFolderOf(path: string): string | null {
    const base = root();
    if (base === null || !isSameOrInside(path, base)) return null;
    let current = path;
    for (;;) {
      if (asked(current)) return current;
      if (current === base) return null;
      const parent = dirname(current);
      if (parent === current || !isSameOrInside(parent, base)) return null;
      current = parent;
    }
  }

  /** A file another program changed: its folder is listed again. */
  function handleChanged(path: string, removed: boolean): void {
    if (removed && asked(path)) {
      setDirs(
        produce((held) => {
          for (const listed of Object.keys(held)) {
            if (isSameOrInside(listed, path)) delete held[listed];
          }
        }),
      );
      setErrors(
        produce((held) => {
          for (const failed of Object.keys(held)) {
            if (isSameOrInside(failed, path)) delete held[failed];
          }
        }),
      );
    }
    const folder = listedFolderOf(path === root() ? path : dirname(path));
    if (folder !== null) void loadDir(folder);
  }

  /**
   * Writ wrote files itself: each folder holding one is listed again, once
   * however many of its files the write named. A path naming the notes folder
   * itself (a walk, a move) lists the folder again.
   */
  function handleIndexed(paths: readonly string[]): void {
    const folders = new Set<string>();
    for (const path of paths) {
      const folder = listedFolderOf(path);
      if (folder !== null) folders.add(folder);
    }
    for (const folder of folders) void loadDir(folder);
  }

  async function reset(): Promise<void> {
    forgetAll();
    loading.clear();
    const held = subscriptions;
    subscriptions = null;
    if (held) for (const unlisten of await Promise.all(held)) unlisten();
  }

  return { root, entriesFor, errorFor, loadDir, reset };
}

export const notesTreeStore = createRoot(createNotesTreeStore);
