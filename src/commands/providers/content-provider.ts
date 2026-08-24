import { searchBuffers } from "../../services/tauri";
import { workspaceStore } from "../../stores/global/workspace";
import { workspaceSearchStore } from "../../stores/global/workspace-search";
import { basename, joinPath, pathKey } from "../../lib/path";
import { bufferPathKeys, bufferTargets, openTarget } from "./open-target";
import type { PaletteResult, ResultProvider } from "../../components/Palette/types";

export const CONTENT_DEBOUNCE_MS = 120;

export interface ContentProviderOptions {
  order?: number;
  cap?: number;
}

// Buffer content comes from the buffer index (`search_buffers`); workspace
// content is streamed by the grep engine. A workspace file that is also an open
// buffer is dropped from the grep side: the buffer copy is the one with unsaved
// state, and it is already covered by the buffer pass.
export function createContentProvider(options: ContentProviderOptions = {}): ResultProvider {
  return {
    id: "content",
    section: "Content",
    order: options.order ?? 3,
    cap: options.cap ?? 12,
    modes: ["all", "content"],
    debounceMs: CONTENT_DEBOUNCE_MS,
    async query(q: string): Promise<PaletteResult[]> {
      if (!q) return [];
      const targets = bufferTargets();
      const kindById = new Map(targets.map((t) => [t.doc.id, t.kind]));
      // A rejection travels to the palette, which is the surface that can tell
      // the user this section is missing rather than empty.
      const results = await searchBuffers(q);
      return results.hits
        .filter((hit) => kindById.has(hit.buffer_id))
        .map((hit) => ({
          id: `content:buffer:${hit.buffer_id}:${hit.line ?? 0}`,
          label: hit.title,
          snippet: hit.snippet,
          line: hit.line ?? undefined,
          execute: () =>
            openTarget(
              { kind: kindById.get(hit.buffer_id)!, id: hit.buffer_id },
              hit.line ?? undefined,
            ),
        }));
    },
    async stream(q, onBatch, signal) {
      if (!q) return;
      const root = workspaceStore.root();
      if (!root) return;
      const skip = bufferPathKeys(bufferTargets());

      await workspaceSearchStore.streamContent(
        q,
        (batch) => {
          const rows: PaletteResult[] = [];
          for (const hit of batch.hits) {
            const absolute = joinPath(root, hit.path);
            if (skip.has(pathKey(absolute))) continue;
            rows.push({
              id: `content:workspace:${hit.path}:${hit.line}`,
              label: basename(hit.path),
              detail: hit.path,
              snippet: hit.snippet,
              line: hit.line,
              execute: () => openTarget({ kind: "file", path: absolute }, hit.line),
            });
          }
          if (rows.length > 0) onBatch(rows);
        },
        signal,
      );
    },
  };
}
