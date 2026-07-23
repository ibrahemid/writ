import { EditorSelection, type SelectionRange } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";

type Change = { from: number; to?: number; insert: string };

const LEADING_WS = /^[ \t]*/;
const TRAILING_WS = /\s+$/;
const LEADING_ANY_WS = /^\s*/;

/**
 * Duplicates each selection. A non-empty range is copied immediately after
 * itself and the copy is selected; a bare cursor copies its line below and
 * keeps the column. Every change is computed against the original document and
 * dispatched as one transaction, so undo is a single step and multiple cursors
 * stay correct. Returns `false` on a read-only view.
 */
export function duplicateSelectionOrLine(view: EditorView): boolean {
  const { state } = view;
  if (state.readOnly) return false;

  const changes: Change[] = [];
  const selections: SelectionRange[] = [];
  let shift = 0;

  for (const range of state.selection.ranges) {
    if (range.empty) {
      const line = state.doc.lineAt(range.head);
      const col = range.head - line.from;
      const insert = "\n" + line.text;
      changes.push({ from: line.to, insert });
      // The copy begins right after the inserted newline; earlier ranges have
      // already pushed everything after them right by `shift`.
      const copyStart = line.to + shift + 1;
      selections.push(EditorSelection.cursor(copyStart + col));
      shift += insert.length;
    } else {
      const text = state.sliceDoc(range.from, range.to);
      changes.push({ from: range.to, insert: text });
      const copyStart = range.to + shift;
      selections.push(EditorSelection.range(copyStart, copyStart + text.length));
      shift += text.length;
    }
  }

  view.dispatch({
    changes,
    selection: EditorSelection.create(selections),
    scrollIntoView: true,
    userEvent: "input.copyline",
  });
  return true;
}

/**
 * Inserts a blank line above the reference line of each selection, carrying the
 * reference line's indentation, and lands the cursor on the new line at that
 * indentation. Mirrors CodeMirror's `insertBlankLine`, upward.
 */
export function insertBlankLineAbove(view: EditorView): boolean {
  const { state } = view;
  if (state.readOnly) return false;

  const changes: Change[] = [];
  const selections: SelectionRange[] = [];
  let shift = 0;

  for (const range of state.selection.ranges) {
    const line = state.doc.lineAt(range.head);
    const indent = LEADING_WS.exec(line.text)![0];
    const insert = indent + "\n";
    changes.push({ from: line.from, insert });
    selections.push(EditorSelection.cursor(line.from + shift + indent.length));
    shift += insert.length;
  }

  view.dispatch({
    changes,
    selection: EditorSelection.create(selections),
    scrollIntoView: true,
    userEvent: "input",
  });
  return true;
}

/**
 * Joins lines. A bare cursor joins its line with the next; a non-empty range
 * joins every line it spans. Each boundary collapses to a single space, the
 * joined line's leading indentation is dropped, and trailing whitespace on the
 * preceding line is collapsed. Returns `false` when there is nothing to join (a
 * bare cursor on the last line).
 */
export function joinLines(view: EditorView): boolean {
  const { state } = view;
  if (state.readOnly) return false;
  const { doc } = state;

  const changes: Change[] = [];
  const seen = new Set<number>();

  for (const range of state.selection.ranges) {
    const startLine = doc.lineAt(range.from).number;
    let endLine = doc.lineAt(range.to).number;
    if (range.empty) {
      if (startLine >= doc.lines) continue;
      endLine = startLine + 1;
    }
    for (let n = startLine; n < endLine; n++) {
      if (seen.has(n)) continue;
      seen.add(n);
      const a = doc.line(n);
      const b = doc.line(n + 1);
      const trimmedEnd = a.from + a.text.replace(TRAILING_WS, "").length;
      const leadLen = LEADING_ANY_WS.exec(b.text)![0].length;
      changes.push({ from: trimmedEnd, to: b.from + leadLen, insert: " " });
    }
  }

  if (changes.length === 0) return false;

  const changeSet = state.changes(changes);
  view.dispatch({
    changes: changeSet,
    selection: state.selection.map(changeSet),
    scrollIntoView: true,
    userEvent: "input.joinline",
  });
  return true;
}
