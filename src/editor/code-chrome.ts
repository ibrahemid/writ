import type { Extension } from "@codemirror/state";
import { lineNumbers, highlightActiveLine, highlightActiveLineGutter } from "@codemirror/view";

/**
 * A buffer is code when a language other than markdown was detected. A
 * markdown note and a buffer with no language are prose.
 *
 * The mono face (`writCodeFace`) and the chrome below both hang off this one
 * predicate, so the two can never disagree about what a note is.
 */
export function isCodeBuffer(lang: string | null): boolean {
  return lang !== null && lang !== "markdown";
}

/**
 * Line numbers and the active-row highlight: what a source file is read with,
 * and what a note is not. The prose surface is one reading column with an
 * empty left margin (ADR-030 decision 3), so a prose buffer gets neither the
 * gutter nor the active-line background.
 */
export const codeChrome: Extension = [
  lineNumbers(),
  highlightActiveLineGutter(),
  highlightActiveLine(),
];

export function codeChromeFor(lang: string | null): Extension {
  return isCodeBuffer(lang) ? codeChrome : [];
}
