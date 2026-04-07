import { EditorSelection } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";

function addCursorVertically(view: EditorView, direction: -1 | 1): boolean {
  const { state } = view;
  const newRanges = [];

  for (const range of state.selection.ranges) {
    const line = state.doc.lineAt(range.head);
    const targetLineNumber = line.number + direction;

    if (targetLineNumber < 1 || targetLineNumber > state.doc.lines) {
      continue;
    }

    const targetLine = state.doc.line(targetLineNumber);
    const col = range.head - line.from;
    const clampedHead = Math.min(targetLine.from + col, targetLine.to);
    newRanges.push(EditorSelection.cursor(clampedHead));
  }

  if (newRanges.length === 0) {
    return false;
  }

  const merged = EditorSelection.create(
    [...state.selection.ranges, ...newRanges],
    state.selection.mainIndex,
  );

  view.dispatch({
    selection: merged,
    scrollIntoView: true,
  });

  return true;
}

export function addCursorUp(view: EditorView): boolean {
  return addCursorVertically(view, -1);
}

export function addCursorDown(view: EditorView): boolean {
  return addCursorVertically(view, 1);
}
