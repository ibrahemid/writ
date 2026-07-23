// A single spell-check finding, in CodeMirror 6-native UTF-16 coordinates.
// Mirrors `writ_lint::LintResult` (serialized camelCase).
export interface SpellingLint {
  fromUtf16: number;
  toUtf16: number;
  kind: string;
  message: string;
  suggestions: string[];
  confident: boolean;
}
