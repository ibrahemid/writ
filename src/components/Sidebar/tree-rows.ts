import { LIST_FAILED } from "../../stores/global/folder-listing";
import type { WorkspaceEntry } from "../../types/workspace";

/** A line the tree draws under a folder in place of, or after, its entries. */
export type TreeStatus = "failed" | "empty" | "unreadable";

/**
 * One row of the folder tree as it is drawn: the tree flattened, a folder's
 * entries straight after it while it is open. `index` is the row's place in
 * the flat list; `posinset` and `setsize` its place among its siblings, which
 * a flat list has to say itself (WAI-ARIA tree, no groups).
 */
export type TreeRow =
  | {
      kind: "entry";
      key: string;
      index: number;
      entry: WorkspaceEntry;
      level: number;
      /** The folder the entry is in. */
      parent: string;
      posinset: number;
      setsize: number;
      expanded: boolean;
    }
  | {
      kind: "status";
      key: string;
      index: number;
      status: TreeStatus;
      /** The folder the line is about. */
      dir: string;
      level: number;
      parent: string;
      /** Entries left out, for `unreadable`. */
      count: number;
    };

/** What the tree is read from. */
export interface TreeSource {
  entriesFor(path: string): WorkspaceEntry[] | undefined;
  errorFor(path: string): string | null;
  unreadableFor(path: string): number;
  isExpanded(path: string): boolean;
}

/**
 * Whether the selected tag names `entry`: the note itself, or a note anywhere
 * under a folder.
 *
 * A folder is kept for what it holds, which is a prefix test rather than a
 * walk: a folder the user has never opened keeps the notes under it in the
 * list without the tree loading a single directory to find out.
 */
export function namesEntry(paths: ReadonlySet<string>, entry: WorkspaceEntry): boolean {
  if (!entry.is_dir) return paths.has(entry.path);
  const separator = entry.path.includes("\\") ? "\\" : "/";
  const prefix = entry.path.endsWith(separator) ? entry.path : entry.path + separator;
  for (const path of paths) {
    if (path.startsWith(prefix)) return true;
  }
  return false;
}

/** `rows` under the tag filter, or all of them when no tag is selected. */
export function underFilter(
  rows: readonly WorkspaceEntry[],
  paths: ReadonlySet<string> | null,
): readonly WorkspaceEntry[] {
  return paths === null ? rows : rows.filter((entry) => namesEntry(paths, entry));
}

/**
 * The rows of the tree under `root`, top to bottom.
 *
 * Under a tag filter (`tagged` not `null`) a folder is in the list only
 * because a note under it carries the tag, so it is drawn open rather than
 * leaving that note behind a caret; clearing the tag hands the folder back
 * whatever the person had.
 *
 * Under an open folder the tree says what it knows: entries it could not read
 * are counted on a line after the rest, a folder that could not be listed says
 * so, and an empty one says it is empty. A folder still being read for the
 * first time draws nothing under it. The root's own failure and emptiness are
 * the caller's to draw, outside the rows.
 */
export function flattenTree(
  root: string,
  source: TreeSource,
  tagged: ReadonlySet<string> | null,
): TreeRow[] {
  const rows: TreeRow[] = [];

  function status(dir: string, level: number, kind: TreeStatus, count = 0): void {
    rows.push({
      kind: "status",
      key: `${kind}:${dir}`,
      index: rows.length,
      status: kind,
      dir,
      level,
      parent: dir,
      count,
    });
  }

  function walk(dir: string, level: number, isRoot: boolean): void {
    const listed = source.entriesFor(dir);
    const children = underFilter(listed ?? [], tagged);
    for (let i = 0; i < children.length; i++) {
      const entry = children[i];
      const expanded = entry.is_dir && (tagged !== null || source.isExpanded(entry.path));
      rows.push({
        kind: "entry",
        key: entry.path,
        index: rows.length,
        entry,
        level,
        parent: dir,
        posinset: i + 1,
        setsize: children.length,
        expanded,
      });
      if (expanded) walk(entry.path, level + 1, false);
    }
    if (isRoot) {
      const skipped = source.unreadableFor(dir);
      if (tagged === null && skipped > 0) status(dir, level, "unreadable", skipped);
      return;
    }
    if (source.errorFor(dir) !== null) {
      status(dir, level, "failed");
      return;
    }
    if (tagged !== null || listed === undefined) return;
    const skipped = source.unreadableFor(dir);
    if (skipped > 0) status(dir, level, "unreadable", skipped);
    else if (children.length === 0) status(dir, level, "empty");
  }

  walk(root, 1, true);
  return rows;
}

/** The line a status row reads. */
export function statusLine(row: Extract<TreeRow, { kind: "status" }>): string {
  switch (row.status) {
    case "failed":
      return LIST_FAILED;
    case "empty":
      return "Empty folder";
    case "unreadable":
      return row.count === 1 ? "Could not read 1 item" : `Could not read ${row.count} items`;
  }
}

/**
 * The row after `from` whose name starts with `prefix`, wrapping to the top;
 * `null` when no row does. Status lines are skipped: they have no name.
 */
export function findByPrefix(
  rows: readonly TreeRow[],
  from: number,
  prefix: string,
): number | null {
  const wanted = prefix.toLocaleLowerCase();
  for (let step = 1; step <= rows.length; step++) {
    const at = (from + step) % rows.length;
    const row = rows[at];
    if (row.kind === "entry" && row.entry.name.toLocaleLowerCase().startsWith(wanted)) return at;
  }
  return null;
}
