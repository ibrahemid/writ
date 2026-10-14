import { createEffect, createRoot, on } from "solid-js";
import * as tauri from "../../services/tauri";
import { onEvent, type UnlistenFn } from "../../services/events";
import { dirname, isSameOrInside } from "../../lib/path";
import { createFolderListing } from "./folder-listing";
import { notesStore } from "./notes";

// Singleton state — Writ is single-window. The listing of the notes folder the
// sidebar draws when no other folder is open (NV-01): one array per folder the
// tree has opened, read from disk and kept in step with the folder by the
// three events that say it changed.

function createNotesTreeStore() {
  let subscriptions: Promise<UnlistenFn>[] | null = null;

  function root(): string | null {
    return notesStore.root();
  }

  const listing = createFolderListing((path) => tauri.listNotesDir(path), root);

  // A moved notes folder is another folder: nothing listed under the old one
  // describes it.
  createEffect(on(notesStore.root, () => listing.forgetAll(), { defer: true }));

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
        for (const path of listing.askedPaths()) void loadDir(path);
      }),
    ];
  }

  /** Lists `path` from disk ([`FolderListing.loadDir`]). */
  function loadDir(path: string): Promise<void> {
    subscribe();
    return listing.loadDir(path);
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
      if (listing.asked(current)) return current;
      if (current === base) return null;
      const parent = dirname(current);
      if (parent === current || !isSameOrInside(parent, base)) return null;
      current = parent;
    }
  }

  /** A file another program changed: its folder is listed again. */
  function handleChanged(path: string, removed: boolean): void {
    if (removed && listing.asked(path)) listing.forgetUnder(path);
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
    listing.forgetAll();
    const held = subscriptions;
    subscriptions = null;
    if (held) for (const unlisten of await Promise.all(held)) unlisten();
  }

  async function revealEntry(path: string): Promise<void> {
    await tauri.showNotesFileInFileManager(path);
  }

  return {
    root,
    entriesFor: listing.entriesFor,
    errorFor: listing.errorFor,
    unreadableFor: listing.unreadableFor,
    isLoading: listing.isLoading,
    loadDir,
    revealEntry,
    reset,
  };
}

export const notesTreeStore = createRoot(createNotesTreeStore);
