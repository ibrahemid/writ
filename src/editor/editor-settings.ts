import { EditorState, type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { indentUnit } from "@codemirror/language";

// The range Settings, Editor offers for Tab size. A config written by hand can
// hold anything, so the editor holds the value to the same range.
export const TAB_SIZE_MIN = 1;
export const TAB_SIZE_MAX = 16;

export function clampTabSize(size: number): number {
  if (!Number.isFinite(size)) return TAB_SIZE_MIN;
  return Math.min(TAB_SIZE_MAX, Math.max(TAB_SIZE_MIN, Math.round(size)));
}

/**
 * Soft wrapping, from Settings, Editor, Word wrap, where the file's tier
 * allows it (`allowed`, from [`featuresFor`]): a large or binary file, and a
 * long-lined one whose longest line is past what wrapping measures in its
 * budget, do not wrap unless the person turned it on for that file.
 */
export function lineWrappingFor(wordWrap: boolean, allowed: boolean): Extension {
  return wordWrap && allowed ? EditorView.lineWrapping : [];
}

/**
 * How wide a tab is drawn and how far an indent goes, from Settings, Editor,
 * Tab size. Every mode follows it: a tab's width costs nothing to change.
 */
export function indentationFor(tabSize: number): Extension {
  const size = clampTabSize(tabSize);
  return [EditorState.tabSize.of(size), indentUnit.of(" ".repeat(size))];
}
