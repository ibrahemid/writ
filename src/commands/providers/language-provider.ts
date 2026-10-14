import { windowRegistry } from "../../stores/global/window-registry";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { PLAIN_TEXT_LANGUAGE, isKnownLanguage, languageChoices } from "../../editor/languages";
import type { PaletteResult, ResultProvider } from "../../components/Palette/types";
import { showToast } from "../../stores/global/notifications";
import { displayFileName } from "../../lib/display-name";
import { basename } from "../../lib/path";
import { logFailure } from "../../lib/log";
import type { BufferDocument } from "../../types/buffer";

/** One row of the language list: what it sets the file's choice to. */
interface LanguageRow {
  /** The choice the row records; null lets the file's name decide again. */
  choice: string | null;
  label: string;
  /** Other words the row is found by: the language's aliases. */
  terms: readonly string[];
}

const DETECT_LABEL = "Detect automatically";
const PLAIN_LABEL = "Plain text";

// Every language the editor knows, plus the two choices that are not one.
// Capped above the catalog's size, so no language is ever cut from the list.
const ROW_CAP = 400;

function rows(): LanguageRow[] {
  return [
    { choice: null, label: DETECT_LABEL, terms: ["auto", "automatic", "detect"] },
    { choice: PLAIN_TEXT_LANGUAGE, label: PLAIN_LABEL, terms: ["plain", "text", "txt", "none"] },
    ...languageChoices().map((choice) => ({
      choice: choice.id,
      label: choice.name,
      terms: choice.aliases,
    })),
  ];
}

// Higher is better; -1 leaves the row out. Ties keep the list's own order.
function rank(row: LanguageRow, query: string): number {
  if (!query) return 0;
  const label = row.label.toLowerCase();
  if (label === query) return 6;
  if (label.startsWith(query)) return 5;
  if (row.terms.includes(query)) return 4;
  if (label.includes(query)) return 3;
  if (row.terms.some((term) => term.startsWith(query))) return 2;
  if (row.terms.some((term) => term.includes(query))) return 1;
  return -1;
}

// The choice a row stands for is what the file holds now. A stored id the
// editor does not know counts as no choice, which is how the editor treats it.
function currentChoice(doc: BufferDocument): string | null {
  const stored = doc.language;
  if (stored === null) return null;
  if (stored === PLAIN_TEXT_LANGUAGE || isKnownLanguage(stored)) return stored;
  return null;
}

function fileName(doc: BufferDocument): string {
  return displayFileName(doc.source_path ? basename(doc.source_path) : doc.title);
}

async function choose(doc: BufferDocument, choice: string | null): Promise<void> {
  try {
    await bufferRegistry.setLanguage(doc.id, choice);
  } catch {
    logFailure("a file's language could not be changed");
    showToast(`Couldn't change the language of ${fileName(doc)}`, "error");
  }
}

/**
 * The language list behind Change Language: the file in front is shown in the
 * language a row names, and the choice is kept with the file.
 */
export function createLanguageProvider(): ResultProvider {
  return {
    id: "language",
    section: "Languages",
    heading: null,
    order: 0,
    cap: ROW_CAP,
    query(q: string): PaletteResult[] {
      const win = windowRegistry.getActive();
      const activeId = win?.tabs.activeTabId() ?? null;
      if (!activeId) return [];
      const doc = bufferRegistry.activeTabs().find((b) => b.id === activeId);
      if (!doc) return [];
      const current = currentChoice(doc);
      const query = q.trim().toLowerCase();
      return rows()
        .map((row) => ({ row, score: rank(row, query) }))
        .filter(({ score }) => score >= 0)
        .sort((a, b) => b.score - a.score)
        .map(({ row }) => ({
          id: `language:${row.choice ?? "detect"}`,
          label: row.label,
          detail: row.choice === current ? "Current" : undefined,
          execute: () => void choose(doc, row.choice),
        }));
    },
  };
}
