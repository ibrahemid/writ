import { displayFileName } from "./display-name";
import { basename, pathKey } from "./path";
import { pathInRoots } from "./tab-labels";

export interface LinkCandidate {
  path: string;
  name: string;
  folder: string;
}

export const LINK_SUGGESTION_LIMIT = 20;

const LINKABLE = /\.(?:md|markdown|txt|text)$/i;

export function isLinkable(path: string, notesRoot: string | null): boolean {
  if (!notesRoot || !LINKABLE.test(path)) return false;
  return pathInRoots(path, [notesRoot]) !== path;
}

function candidateFor(path: string, notesRoot: string | null): LinkCandidate {
  const relative = notesRoot ? pathInRoots(path, [notesRoot]) : path;
  const cut = relative.lastIndexOf("/");
  return {
    path,
    name: displayFileName(basename(path)),
    folder: cut === -1 ? "" : relative.slice(0, cut),
  };
}

export function emptyQueryLinks(
  recent: readonly string[],
  nearby: readonly string[],
  fromPath: string | null,
  notesRoot: string | null,
  limit = LINK_SUGGESTION_LIMIT,
): LinkCandidate[] {
  const seen = new Set<string>(fromPath ? [pathKey(fromPath)] : []);
  const out: LinkCandidate[] = [];
  for (const path of [...recent, ...nearby]) {
    if (out.length >= limit) break;
    const key = pathKey(path);
    if (seen.has(key) || !isLinkable(path, notesRoot)) continue;
    seen.add(key);
    out.push(candidateFor(path, notesRoot));
  }
  return out;
}
