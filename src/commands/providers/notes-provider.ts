import { searchNotesByName } from "../../services/tauri";
import { pathKey } from "../../lib/path";
import { bufferPathKeys, bufferTargets, openTarget } from "./open-target";
import type { PaletteResult, ResultProvider } from "../../components/Palette/types";

export const NOTES_DEBOUNCE_MS = 120;

export interface NotesProviderOptions {
  order?: number;
  cap?: number;
}

// Notes by name, from the path-keyed index (ADR-028 section 7). A note that is
// also an open tab renders once, as the tab, because that copy carries unsaved
// state; everything else opens by its path.
export function createNotesProvider(options: NotesProviderOptions = {}): ResultProvider {
  return {
    id: "notes",
    section: "Notes",
    order: options.order ?? 1,
    cap: options.cap ?? 12,
    modes: ["notes"],
    debounceMs: NOTES_DEBOUNCE_MS,
    async query(q: string): Promise<PaletteResult[]> {
      if (!q) return [];

      const targets = bufferTargets();
      const openByPath = new Map(
        targets
          .filter((t) => t.doc.source_path)
          .map((t) => [pathKey(t.doc.source_path as string), t]),
      );
      const taken = bufferPathKeys(targets);

      const hits = await searchNotesByName(q);
      const results: PaletteResult[] = [];
      for (const hit of hits) {
        const key = pathKey(hit.path);
        const open = openByPath.get(key);
        if (open) {
          results.push({
            id: `note:buffer:${open.doc.id}`,
            label: hit.name,
            detail: hit.path,
            execute: () => openTarget({ kind: open.kind, id: open.doc.id }),
          });
          continue;
        }
        if (taken.has(key)) continue;
        results.push({
          id: `note:file:${hit.path}`,
          label: hit.name,
          detail: hit.path,
          execute: () => openTarget({ kind: "file", path: hit.path }),
        });
      }
      return results;
    },
  };
}
