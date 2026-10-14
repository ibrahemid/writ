import { configStore } from "./config";
import { notesStore } from "./notes";
import { notesTreeStore } from "./notes-tree";
import { workspaceStore } from "./workspace";
import type { WorkspaceEntry } from "../../types/workspace";

// Singleton state — Writ is single-window. Which folder the sidebar's Files
// section draws, and whether that tree can show what a tag names (NV-01,
// NV-02). Derived from the folder opened by hand and the notes folder; it
// holds nothing of its own.

/** A folder the Files section can draw, read a level at a time. */
export interface FolderTree {
  root: () => string | null;
  entriesFor: (path: string) => WorkspaceEntry[] | undefined;
  loadDir: (path: string) => Promise<void>;
  /** Why a folder could not be listed, for a tree that says so. */
  errorFor: (path: string) => string | null;
  /** How many entries of a folder its last listing could not read. */
  unreadableFor: (path: string) => number;
  /** Whether a listing of a folder is running. */
  isLoading: (path: string) => boolean;
  /** Shows an entry in the platform's file manager. */
  revealEntry: (path: string) => Promise<void>;
}

export type FolderKind = "workspace" | "notes";

/**
 * The folder the Files section shows: the one opened by hand while there is
 * one, the notes folder otherwise, and nothing before either is known.
 */
function kind(): FolderKind | null {
  if (workspaceStore.root() !== null) return "workspace";
  if (notesStore.root() !== null) return "notes";
  return null;
}

/** The tree of [`kind`], the same object for as long as the kind holds. */
function tree(): FolderTree | null {
  switch (kind()) {
    case "workspace":
      return workspaceStore;
    case "notes":
      return notesTreeStore;
    default:
      return null;
  }
}

/**
 * Whether the tree on screen holds every file a tag can name, so selecting a
 * tag filters it.
 *
 * Tags come from the notes index, and the notes folder's own tree is listed by
 * the index's rule, so only that tree is filtered. A folder opened by hand is
 * drawn whole, the notes folder itself included, and a Files section switched
 * off or folded shows no tree at all: in each of those the tagged files are
 * listed under the tag instead.
 */
function showsNotes(): boolean {
  return (
    !configStore.isSidebarSectionHidden("folder") &&
    !configStore.isSidebarSectionCollapsed("folder") &&
    kind() === "notes"
  );
}

export const sidebarFolderStore = { kind, tree, showsNotes };
