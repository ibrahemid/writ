import { createStore, produce, reconcile } from "solid-js/store";
import { isSameOrInside } from "../../lib/path";
import type { DirListing, WorkspaceEntry } from "../../types/workspace";

/** What a folder that could not be listed says, in the tree, above Try again. */
export const LIST_FAILED = "Could not read this folder.";

/**
 * The listings of one folder tree, read a level at a time: each folder the
 * tree has opened, whether its read failed, how many of its entries could not
 * be read, and whether a read is running. The sidebar's two trees (the notes
 * folder and a folder opened by hand) each hold one.
 */
export interface FolderListing {
  entriesFor(path: string): WorkspaceEntry[] | undefined;
  /** Why `path` could not be listed, or `null`. */
  errorFor(path: string): string | null;
  /** How many entries of `path` its last listing could not read. */
  unreadableFor(path: string): number;
  /** Whether a listing of `path` is running. */
  isLoading(path: string): boolean;
  /** Whether `path` is listed, or its listing failed. */
  asked(path: string): boolean;
  /** Every folder the tree has asked for. */
  askedPaths(): string[];
  /**
   * Lists `path`. Entries are matched to the ones already listed by path, so
   * a folder open in the tree stays open when the folder around it is listed
   * again. A listing asked for while one is running is owed one more listing
   * after it. A failed listing keeps what was listed before and says why.
   */
  loadDir(path: string): Promise<void>;
  /** Forgets `path` and every folder under it. */
  forgetUnder(path: string): void;
  forgetAll(): void;
}

export function createFolderListing(
  list: (path: string) => Promise<DirListing>,
  root: () => string | null,
): FolderListing {
  const [dirs, setDirs] = createStore<Record<string, WorkspaceEntry[] | undefined>>({});
  const [errors, setErrors] = createStore<Record<string, string | undefined>>({});
  const [unreadable, setUnreadable] = createStore<Record<string, number | undefined>>({});
  const [loading, setLoading] = createStore<Record<string, boolean | undefined>>({});
  const running = new Map<string, Promise<void>>();
  const owed = new Set<string>();

  function loadDir(path: string): Promise<void> {
    const current = running.get(path);
    if (current) {
      owed.add(path);
      return current;
    }
    const base = root();
    setLoading(path, true);
    const run = (async () => {
      try {
        do {
          owed.delete(path);
          try {
            const listing = await list(path);
            if (root() !== base) return;
            setDirs(path, reconcile(listing.entries, { key: "path" }));
            setUnreadable(path, listing.unreadable > 0 ? listing.unreadable : undefined);
            setErrors(path, undefined);
          } catch {
            if (root() !== base) return;
            setErrors(path, LIST_FAILED);
          }
        } while (owed.has(path));
      } finally {
        running.delete(path);
        setLoading(path, undefined);
      }
    })();
    running.set(path, run);
    return run;
  }

  function dropUnder<T>(path: string) {
    return produce((held: Record<string, T | undefined>) => {
      for (const listed of Object.keys(held)) {
        if (isSameOrInside(listed, path)) delete held[listed];
      }
    });
  }

  return {
    entriesFor: (path) => dirs[path],
    errorFor: (path) => errors[path] ?? null,
    unreadableFor: (path) => unreadable[path] ?? 0,
    isLoading: (path) => loading[path] === true,
    asked: (path) => dirs[path] !== undefined || errors[path] !== undefined,
    askedPaths: () => [...new Set([...Object.keys(dirs), ...Object.keys(errors)])],
    loadDir,
    forgetUnder(path) {
      setDirs(dropUnder<WorkspaceEntry[]>(path));
      setErrors(dropUnder<string>(path));
      setUnreadable(dropUnder<number>(path));
    },
    forgetAll() {
      setDirs(reconcile({}));
      setErrors(reconcile({}));
      setUnreadable(reconcile({}));
      owed.clear();
    },
  };
}
