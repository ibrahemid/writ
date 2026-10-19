import type { Text } from "@codemirror/state";
import { detectLanguage, MAX_DETECT_BYTES, textMayNameLanguage } from "../services/language-detect";
import { PLAIN_TEXT_LANGUAGE, isKnownLanguage, languageForFileName } from "./languages";

/** What the language of a file is decided from. */
export interface FileLanguageInputs {
  /** The language chosen for the file with Change Language, or null. */
  chosen: string | null;
  /** The file's name: its path's last part, or the title of a file with no path. */
  name: string;
  /** Whether the file is on disk yet. */
  hasFile: boolean;
  /** The text, for the first line of a name with no extension. */
  content: string;
}

/**
 * The language the editor shows a file in, or null for plain text.
 *
 * A language chosen for the file wins; `plaintext` is a choice too. Otherwise
 * the name decides: Writ's own map, plain text for `.txt` and `.text`, then
 * the editor's language table. Only a name with no extension lets the text
 * speak, through a shebang. An id nobody knows is no choice.
 */
export function languageOfFile(inputs: FileLanguageInputs): string | null {
  const { chosen, name, hasFile, content } = inputs;
  if (chosen !== null) {
    if (chosen === PLAIN_TEXT_LANGUAGE) return null;
    if (isKnownLanguage(chosen)) return chosen;
  }
  if (!hasFile) return languageOfUntitled(name, content);
  return detectLanguage(content, name) ?? languageForFileName(name);
}

/**
 * As much of `doc` as [`languageOfFile`] reads, its `content`: a bounded start,
 * so a document of one long line is not read whole.
 */
export function languageHeadOf(doc: Text): string {
  return doc.sliceString(0, MAX_DETECT_BYTES);
}

/**
 * Whether typing or pasting may give the file a language from what it holds:
 * nobody chose one, and its name says nothing.
 */
export function contentMayNameLanguage(inputs: Pick<FileLanguageInputs, "chosen" | "name">): boolean {
  return inputs.chosen === null && textMayNameLanguage(inputs.name);
}

// A file with no path yet keeps a Markdown surface whatever it holds (F05/F17).
// Its name may still be one the person typed (a rename to `main.rs`), and a
// shebang on its first line stands; the `<id>.txt` a row is filed under names
// nothing, so the shebang is read without it.
function languageOfUntitled(name: string, content: string): string | null {
  const byName = detectLanguage("", name) ?? languageForFileName(name);
  if (byName !== null) return byName;
  if (content.startsWith("#!")) return detectLanguage(content);
  return "markdown";
}
