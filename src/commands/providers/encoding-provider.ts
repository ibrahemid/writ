import { windowRegistry } from "../../stores/global/window-registry";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import type { PaletteResult, ResultProvider } from "../../components/Palette/types";
import { showToast } from "../../components/Notifications/Toast";
import { displayFileName } from "../../lib/display-name";
import { basename } from "../../lib/path";
import { logFailure } from "../../lib/log";
import { ERR_READ_FAILED, readFailureCodeOf, readFailureReason } from "../../lib/read-failure";
import {
  ENCODING_CHOICES,
  encodingChoiceOf,
  type EncodingChoice,
} from "../../lib/text-encoding";
import type { BufferDocument } from "../../types/buffer";

// Higher is better; -1 leaves the row out. Ties keep the list's own order.
function rank(row: EncodingChoice, query: string): number {
  if (!query) return 0;
  const label = row.label.toLowerCase();
  const short = row.short.toLowerCase();
  if (short === query || row.name.toLowerCase() === query) return 6;
  if (label.startsWith(query) || short.startsWith(query)) return 5;
  if (row.terms.includes(query)) return 4;
  if (label.includes(query)) return 3;
  if (row.terms.some((term) => term.startsWith(query))) return 2;
  if (row.terms.some((term) => term.includes(query))) return 1;
  return -1;
}

function fileName(doc: BufferDocument): string {
  return displayFileName(doc.source_path ? basename(doc.source_path) : doc.title);
}

async function reopen(doc: BufferDocument, row: EncodingChoice): Promise<void> {
  const win = windowRegistry.getActive();
  // The reopen replaces the tab's text with the file's, so text no file holds
  // yet would go with it.
  if (win?.editor.isDirty(doc.id)) {
    showToast(`Save ${fileName(doc)} before reopening it in another encoding`, "error");
    return;
  }
  try {
    await bufferRegistry.reopenWithEncoding(doc.id, row.name);
  } catch (error) {
    logFailure("a file could not be reopened in another encoding");
    const code = readFailureCodeOf(error) ?? ERR_READ_FAILED;
    showToast(
      `Couldn't reopen ${fileName(doc)} as ${row.short}: ${readFailureReason(code)}`,
      "error",
    );
    return;
  }
  win?.editor.requestExternalReload(doc.id);
}

/**
 * The list behind Reopen with Encoding: the file in front is read again in the
 * encoding a row names, and saves write that encoding from then on (ADR-049).
 */
export function createEncodingProvider(): ResultProvider {
  return {
    id: "encoding",
    section: "Encodings",
    heading: null,
    order: 0,
    cap: ENCODING_CHOICES.length,
    query(q: string): PaletteResult[] {
      const win = windowRegistry.getActive();
      const activeId = win?.tabs.activeTabId() ?? null;
      if (!activeId) return [];
      const doc = bufferRegistry.activeTabs().find((b) => b.id === activeId);
      // A note with no file yet has nothing to read again; it is written in
      // UTF-8 when it gets one.
      if (!doc || doc.encoding === null || !doc.source_path) return [];
      const current = encodingChoiceOf(doc.encoding)?.name;
      const query = q.trim().toLowerCase();
      return ENCODING_CHOICES.map((row) => ({ row, score: rank(row, query) }))
        .filter(({ score }) => score >= 0)
        .sort((a, b) => b.score - a.score)
        .map(({ row }) => ({
          id: `encoding:${row.name}`,
          label: row.label,
          detail: row.name === current ? "Current" : undefined,
          execute: () => void reopen(doc, row),
        }));
    },
  };
}
