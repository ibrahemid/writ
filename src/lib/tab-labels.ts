import { foldersForSharedNames } from "./shared-names";
import { isSameOrInside } from "./path";

export interface TabIdentity {
  id: string;
  source_path: string | null;
}

export function tabFolderHints(tabs: readonly TabIdentity[]): Map<string, string> {
  const paths = tabs
    .map((tab) => tab.source_path)
    .filter((path): path is string => typeof path === "string" && path !== "");
  const byPath = foldersForSharedNames(paths);
  const hints = new Map<string, string>();
  for (const tab of tabs) {
    if (!tab.source_path) continue;
    const folder = byPath.get(tab.source_path);
    if (folder) hints.set(tab.id, folder);
  }
  return hints;
}

export function pathInRoots(path: string, roots: readonly (string | null | undefined)[]): string {
  for (const root of roots) {
    if (!root || path === root || !isSameOrInside(path, root)) continue;
    return path.slice(root.replace(/[\\/]+$/, "").length + 1).replace(/\\/g, "/");
  }
  return path;
}
