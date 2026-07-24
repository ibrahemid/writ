import { workspaceStore } from "../../stores/global/workspace";
import { workspaceSearchStore } from "../../stores/global/workspace-search";
import { joinPath, pathKey } from "../../lib/path";
import { bufferPathKeys, bufferTargets, openTarget } from "./open-target";
import type { PaletteResult, ResultProvider } from "../../components/Palette/types";

export const FILES_DEBOUNCE_MS = 120;

export interface FilesProviderOptions {
  order?: number;
  cap?: number;
}

function matchesBuffer(title: string, sourcePath: string | null, needle: string): boolean {
  if (title.toLowerCase().includes(needle)) return true;
  return sourcePath !== null && sourcePath.toLowerCase().includes(needle);
}

// Open tabs, history and the workspace file-name index in one list, deduped by
// canonical path. A file that is also an open buffer renders once, as the
// buffer, because that copy carries unsaved state.
export function createFilesProvider(options: FilesProviderOptions = {}): ResultProvider {
  return {
    id: "files",
    section: "Files",
    order: options.order ?? 2,
    cap: options.cap ?? 8,
    debounceMs: FILES_DEBOUNCE_MS,
    async query(q: string): Promise<PaletteResult[]> {
      const needle = q.toLowerCase();
      const targets = bufferTargets();
      const results: PaletteResult[] = [];

      for (const target of targets) {
        if (needle && !matchesBuffer(target.doc.title, target.doc.source_path, needle)) continue;
        results.push({
          id: `file:buffer:${target.doc.id}`,
          label: target.doc.title,
          detail: target.doc.source_path ?? (target.kind === "history" ? "History" : "Unsaved"),
          execute: () => openTarget({ kind: target.kind, id: target.doc.id }),
        });
      }

      if (!needle) return results;

      const root = workspaceStore.root();
      if (!root) return results;

      const taken = bufferPathKeys(targets);
      const hits = await workspaceSearchStore.searchFiles(q);
      for (const hit of hits) {
        const absolute = joinPath(root, hit.path);
        const key = pathKey(absolute);
        if (taken.has(key)) continue;
        taken.add(key);
        results.push({
          id: `file:workspace:${hit.path}`,
          label: hit.name,
          detail: hit.path,
          execute: () => openTarget({ kind: "file", path: absolute }),
        });
      }

      return results;
    },
  };
}
