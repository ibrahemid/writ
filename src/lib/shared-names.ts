import { displayFileName } from "./display-name";
import { basename } from "./path";

/** The folders `path` runs through, outermost first, without its file name. */
function foldersOf(path: string): string[] {
  return path
    .split(/[\\/]/)
    .filter((part) => part !== "")
    .slice(0, -1);
}

/**
 * What tells apart the files of a list that show the same name.
 *
 * Each path whose shown name ([`displayFileName`]) another path in `paths`
 * also has maps to the folder it sits in, with as many folders above it as
 * that name's files need to read differently. A path whose name no other path
 * in the list shows has no entry.
 */
export function foldersForSharedNames(paths: readonly string[]): Map<string, string> {
  const byName = new Map<string, string[]>();
  for (const path of paths) {
    const name = displayFileName(basename(path));
    const group = byName.get(name);
    if (group) group.push(path);
    else byName.set(name, [path]);
  }

  const folders = new Map<string, string>();
  for (const group of byName.values()) {
    if (group.length < 2) continue;
    const chains = group.map(foldersOf);
    const deepest = Math.max(...chains.map((chain) => chain.length));
    const suffix = (chain: string[], depth: number) => chain.slice(-depth).join("/");
    let depth = 1;
    while (
      depth < deepest &&
      new Set(chains.map((chain) => suffix(chain, depth))).size < group.length
    ) {
      depth += 1;
    }
    group.forEach((path, index) => {
      const folder = suffix(chains[index], depth);
      if (folder !== "") folders.set(path, folder);
    });
  }
  return folders;
}
