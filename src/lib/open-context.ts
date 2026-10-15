import type { EditorState } from "@codemirror/state";
import type { SelectedText, TextPoint, WindowContext } from "../services/tauri";

/**
 * Most selected text one answer carries, in UTF-16 units. Rust holds the
 * answer to 2 MB of UTF-8 (ADR-053); this keeps a select-all of a large file
 * from crossing IPC whole first.
 */
export const MAX_SELECTED_UNITS = 2 * 1024 * 1024;

/** A position as a 1-based line and a 1-based column counted in characters. */
export function pointAt(state: EditorState, pos: number): TextPoint {
  const line = state.doc.lineAt(pos);
  return { line: line.number, column: [...line.text.slice(0, pos - line.from)].length + 1 };
}

/** The end of a cut that leaves no half of a surrogate pair behind. */
function cutAt(state: EditorState, from: number, end: number): number {
  if (end <= from) return from;
  const last = state.sliceDoc(end - 1, end).charCodeAt(0);
  return last >= 0xd800 && last <= 0xdbff ? end - 1 : end;
}

/**
 * What the main window answers when a program allowed to see the open file
 * asks: the tab in front by id, its non-empty selections with where each
 * starts and ends, the caret, and whether the tab holds unsaved text. The
 * file is Rust's to name from the id.
 */
export function windowContext(
  bufferId: string | null,
  state: EditorState | null,
  unsaved: boolean,
): WindowContext {
  if (bufferId === null || state === null) {
    return { buffer_id: null, selections: [], cursor: null, unsaved: false, truncated: false };
  }
  const selections: SelectedText[] = [];
  let budget = MAX_SELECTED_UNITS;
  let truncated = false;
  for (const range of state.selection.ranges) {
    if (range.empty) continue;
    if (budget === 0) {
      truncated = true;
      break;
    }
    const end = range.to - range.from > budget ? cutAt(state, range.from, range.from + budget) : range.to;
    if (end < range.to) truncated = true;
    if (end === range.from) break;
    selections.push({
      text: state.sliceDoc(range.from, end),
      from: pointAt(state, range.from),
      to: pointAt(state, end),
    });
    budget -= end - range.from;
  }
  return {
    buffer_id: bufferId,
    selections,
    cursor: pointAt(state, state.selection.main.head),
    unsaved,
    truncated,
  };
}
