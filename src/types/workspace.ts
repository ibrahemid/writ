/** Who wrote a second copy of a file beside the original. */
export type ConflictCopyKind = "sync_client" | "writ";

export interface WorkspaceEntry {
  name: string;
  path: string;
  is_dir: boolean;
  /**
   * Set when the name says a sync client, or Writ itself, kept a second copy
   * of another file here. Such a file is listed like any other: it holds text
   * somebody wrote, and hiding it would hide the only copy of that text.
   */
  conflict_copy: ConflictCopyKind | null;
}

/** One folder's listing, and how many entries in it could not be read. */
export interface DirListing {
  entries: WorkspaceEntry[];
  unreadable: number;
}
