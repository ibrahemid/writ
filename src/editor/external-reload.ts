import { Annotation, EditorSelection } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";

/**
 * Marks a transaction as text arriving from the file rather than from typing.
 *
 * The update listener reads it and skips the autosave it would otherwise
 * schedule: the document already equals the file, and writing it back would
 * defeat the cancelled autosave and move `updated_at` for nothing.
 */
export const ExternalReloadTxn = Annotation.define<boolean>();

/** The single change that turns `before` into `after`. */
export interface DocumentChange {
  from: number;
  to: number;
  insert: string;
}

/** Whether cutting `text` at `index` would split a surrogate pair. */
function splitsAPair(text: string, index: number): boolean {
  if (index <= 0 || index >= text.length) return false;
  const high = text.charCodeAt(index - 1);
  const low = text.charCodeAt(index);
  return high >= 0xd800 && high <= 0xdbff && low >= 0xdc00 && low <= 0xdfff;
}

/**
 * The smallest single change that turns `before` into `after`.
 *
 * The two texts are the same file a moment apart, so what differs is usually a
 * line in the middle of a page of untouched text. Replacing only that range is
 * what lets CodeMirror map every position through the change: the cursor, the
 * scroll and the folded ranges above the edit all survive because nothing
 * above the edit moved. Replacing the whole document instead moves every
 * position to the end of the insert and drops the reader where they were not.
 *
 * Neither end is allowed to cut a surrogate pair in half, which is what an
 * edit next to an emoji would otherwise do.
 */
export function smallestChange(before: string, after: string): DocumentChange {
  const max = Math.min(before.length, after.length);
  let prefix = 0;
  while (prefix < max && before.charCodeAt(prefix) === after.charCodeAt(prefix)) {
    prefix++;
  }
  if (splitsAPair(before, prefix)) prefix--;

  let suffix = 0;
  while (
    suffix < max - prefix &&
    before.charCodeAt(before.length - 1 - suffix) ===
      after.charCodeAt(after.length - 1 - suffix)
  ) {
    suffix++;
  }
  if (splitsAPair(before, before.length - suffix)) suffix--;

  return {
    from: prefix,
    to: before.length - suffix,
    insert: after.slice(prefix, after.length - suffix),
  };
}

/** The offset of `line` (1-based) in `text`, clamped to the last line. */
function lineStart(text: string, line: number): number {
  let offset = 0;
  for (let n = 1; n < line; n++) {
    const next = text.indexOf("\n", offset);
    if (next === -1) return offset;
    offset = next + 1;
  }
  return offset;
}

/** The offset the cursor takes in `text` for a line and column it held. */
export function positionOnLine(text: string, line: number, column: number): number {
  const from = lineStart(text, line);
  const end = text.indexOf("\n", from);
  const to = end === -1 ? text.length : end;
  return Math.min(from + column, to);
}

/**
 * Replaces the view's document with the file's text in one tracked
 * transaction, leaving the reader where they were.
 *
 * One transaction, so one Cmd+Z puts the document back the way it was before
 * the file changed under it. The cursor keeps its line, or the nearest one a
 * shorter file has, and the scroll position is put back after the dispatch —
 * a reload that jumps a person to the top of a file they were reading in the
 * middle of is a reload they will turn off.
 */
export function applyExternalDocument(view: EditorView, text: string): void {
  const before = view.state.doc.toString();
  if (before === text) return;

  const head = view.state.selection.main.head;
  const line = view.state.doc.lineAt(head);
  const scrollTop = view.scrollDOM.scrollTop;

  const change = smallestChange(before, text);
  const cursor = positionOnLine(text, line.number, head - line.from);

  view.dispatch({
    changes: change,
    selection: EditorSelection.cursor(cursor),
    annotations: ExternalReloadTxn.of(true),
    scrollIntoView: false,
  });

  view.scrollDOM.scrollTop = scrollTop;
}
