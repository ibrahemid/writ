import { LanguageDescription, type LanguageSupport } from "@codemirror/language";
import { languages as LANGUAGE_DATA } from "@codemirror/language-data";
import { extensionOf, namesPlainText } from "../services/language-detect";
import { MARKDOWN_EXTENSIONS } from "./markdown-math";

/**
 * The languages the editor highlights, and how a grammar is found and loaded.
 *
 * Every grammar comes from CodeMirror's `@codemirror/language-data` table and
 * loads the first time a file needs it, so the main chunk carries no grammar
 * and a plain-text file never waits for one. Two descriptions are Writ's own:
 * Markdown, which Writ parses with the GitHub extensions, with math, and with
 * every fence highlighted in its language, and JavaScript, which Writ parses with JSX the
 * way it always has, so a React file named `.js` keeps its tags highlighted.
 *
 * A language is named by an id: the description's name in lower case
 * (`python`, `yaml`, `c++`). The ids the detector returns are names or
 * aliases in the table, so the same lookup serves both.
 */

/** The id a person picks to show a file as plain text, whatever it is named. */
export const PLAIN_TEXT_LANGUAGE = "plaintext";

export class LanguageCatalogError extends Error {
  constructor(name: string) {
    super(`@codemirror/language-data no longer describes ${name}`);
    this.name = "LanguageCatalogError";
  }
}

// A Writ-owned description keeps the table's names, aliases and file patterns,
// so detection and the picker see one entry, and swaps only how it loads.
function ownDescription(
  name: string,
  load: () => Promise<LanguageSupport>,
): LanguageDescription {
  const base = LANGUAGE_DATA.find((description) => description.name === name);
  if (!base) throw new LanguageCatalogError(name);
  return LanguageDescription.of({
    name: base.name,
    alias: base.alias,
    extensions: base.extensions,
    filename: base.filename,
    load,
  });
}

const WRIT_OWNED: readonly LanguageDescription[] = [
  ownDescription("Markdown", () =>
    import("@codemirror/lang-markdown").then((m) =>
      m.markdown({
        base: m.markdownLanguage,
        codeLanguages: LANGUAGE_CATALOG,
        extensions: MARKDOWN_EXTENSIONS,
      }),
    ),
  ),
  ownDescription("JavaScript", () =>
    import("@codemirror/lang-javascript").then((m) => m.javascript({ jsx: true })),
  ),
];

/** Every language the editor can show a file in, Writ's own first. */
export const LANGUAGE_CATALOG: readonly LanguageDescription[] = [
  ...WRIT_OWNED,
  ...LANGUAGE_DATA.filter((description) =>
    WRIT_OWNED.every((own) => own.name !== description.name),
  ),
];

/** The id a description is known by. */
export function languageId(description: LanguageDescription): string {
  return description.name.toLowerCase();
}

/** The description an id names, by name or alias, or null when none does. */
export function grammarFor(id: string | null): LanguageDescription | null {
  if (id === null || id === PLAIN_TEXT_LANGUAGE) return null;
  return LanguageDescription.matchLanguageName(LANGUAGE_CATALOG, id, false);
}

/** Whether an id names a language the editor knows. */
export function isKnownLanguage(id: string): boolean {
  return grammarFor(id) !== null;
}

// Extensions the table gives a language that most files so named are not in:
// `.cfg` reads as TTCN's configuration and `.in` as Java properties, while such
// files are mostly another program's settings or a template of another file.
const EXTENSIONS_THE_TABLE_MISREADS = new Set(["cfg", "in"]);

/**
 * The language a file name names in the table: a whole name (`Dockerfile`,
 * `PKGBUILD`), or an extension Writ's own map does not list (`.c`, `.kt`), in
 * any case.
 *
 * A name that says plain text names nothing, whole names included
 * (`CMakeLists.txt`), and neither does an extension more than one language in
 * the table claims (`.m`): a person picks one.
 */
export function languageForFileName(name: string): string | null {
  const base = name.slice(Math.max(name.lastIndexOf("/"), name.lastIndexOf("\\")) + 1);
  if (namesPlainText(base)) return null;
  const whole = LANGUAGE_CATALOG.find((description) => description.filename?.test(base));
  if (whole) return languageId(whole);
  const ext = extensionOf(base);
  if (ext === null || EXTENSIONS_THE_TABLE_MISREADS.has(ext)) return null;
  const claimants = LANGUAGE_CATALOG.filter((description) =>
    description.extensions.some((own) => own.toLowerCase() === ext),
  );
  return claimants.length === 1 ? languageId(claimants[0]) : null;
}

/** The grammar for an id when it has loaded, else null. */
export function loadedGrammar(id: string | null): LanguageSupport | null {
  return grammarFor(id)?.support ?? null;
}

/**
 * Loads the grammar for an id. Resolves null for an id with none. A grammar
 * loads once; every later call answers from the same load.
 */
export async function loadGrammar(id: string | null): Promise<LanguageSupport | null> {
  const description = grammarFor(id);
  if (!description) return null;
  return description.support ?? description.load();
}

/**
 * Loads a grammar when the main thread is idle, so the first file that needs
 * it does not wait. Markdown is the one worth it: Inline mode draws nothing
 * until its grammar is there.
 */
export function warmGrammar(id: string): void {
  if (loadedGrammar(id)) return;
  const load = () => {
    loadGrammar(id).catch(() => undefined);
  };
  if (typeof requestIdleCallback === "function") requestIdleCallback(load, { timeout: 2000 });
  else setTimeout(load, 500);
}

/** A language as a person picks it. */
export interface LanguageChoice {
  id: string;
  name: string;
  aliases: readonly string[];
}

/** Every language a file can be shown in, by name. */
export function languageChoices(): LanguageChoice[] {
  return LANGUAGE_CATALOG.map((description) => ({
    id: languageId(description),
    name: description.name,
    aliases: description.alias,
  })).sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
}

/** The name a person reads for an id, or null for an id the editor does not know. */
export function languageName(id: string): string | null {
  return grammarFor(id)?.name ?? null;
}
