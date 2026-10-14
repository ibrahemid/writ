import { createStore, produce } from "solid-js/store";

// Singleton state — Writ is single-window. Which folders each sidebar tree has
// open, by the tree's root, kept across launches in the webview's own storage:
// a view preference, so it never reaches the config file. A folder stays open
// when its parent is folded and unfolded again (NV-18).

const STORAGE_KEY = "writ.tree.expanded.v1";
/** Roots remembered; the least recently changed goes first. */
const MAX_ROOTS = 20;
/** Open folders remembered per root; the oldest goes first. */
const MAX_PATHS = 2000;

type Expanded = Record<string, Record<string, true>>;

function load(): Expanded {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return {};
    const out: Expanded = {};
    for (const [root, paths] of Object.entries(parsed as Record<string, unknown>)) {
      if (!Array.isArray(paths)) continue;
      const held: Record<string, true> = {};
      for (const path of paths) if (typeof path === "string") held[path] = true;
      out[root] = held;
    }
    return out;
  } catch {
    return {};
  }
}

const [expanded, setExpandedStore] = createStore<Expanded>(load());
const touched: string[] = Object.keys(expanded);

function persist(): void {
  try {
    const out: Record<string, string[]> = {};
    for (const root of touched) {
      const paths = Object.keys(expanded[root] ?? {});
      if (paths.length > 0) out[root] = paths.slice(-MAX_PATHS);
    }
    localStorage.setItem(STORAGE_KEY, JSON.stringify(out));
  } catch {
    // Storage refused (private mode, quota): the tree still opens and folds,
    // and starts folded next launch.
  }
}

function touch(root: string): void {
  const at = touched.indexOf(root);
  if (at !== -1) touched.splice(at, 1);
  touched.push(root);
  while (touched.length > MAX_ROOTS) {
    const dropped = touched.shift()!;
    setExpandedStore(produce((held) => void delete held[dropped]));
  }
}

function isExpanded(root: string, path: string): boolean {
  return expanded[root]?.[path] === true;
}

function setExpanded(root: string, path: string, open: boolean): void {
  if (isExpanded(root, path) === open) return;
  touch(root);
  setExpandedStore(
    produce((held) => {
      const paths = (held[root] ??= {});
      if (open) paths[path] = true;
      else delete paths[path];
    }),
  );
  persist();
}

export const treeExpansionStore = { isExpanded, setExpanded };
