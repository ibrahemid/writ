import { EditorSelection } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { readClipboardText, writeClipboardText } from "../services/tauri";

/**
 * Clipboard verbs for the editor's context menu.
 *
 * Everything goes through CodeMirror transactions rather than
 * `document.execCommand`, so a cut or paste lands in one undo step and a
 * read-only buffer is respected. Reads and writes use the Tauri clipboard
 * plugin (see `services/tauri`), which behaves the same on all three
 * platforms.
 */

/**
 * Text of every non-empty selection range, joined by line breaks the way
 * CodeMirror's own copy joins them, or "" when nothing is selected.
 */
function selectedText(view: EditorView): string {
  const { state } = view;
  return state.selection.ranges
    .filter((range) => !range.empty)
    .map((range) => state.doc.sliceString(range.from, range.to))
    .join(state.lineBreak);
}

export async function copySelection(view: EditorView): Promise<void> {
  const text = selectedText(view);
  if (!text) return;
  await writeClipboardText(text);
}

export async function cutSelection(view: EditorView): Promise<void> {
  const text = selectedText(view);
  if (!text || view.state.readOnly) return;
  await writeClipboardText(text);
  // Every range goes, each leaving a caret where it started, as Cmd+X does.
  view.dispatch({
    ...view.state.changeByRange((range) =>
      range.empty
        ? { range }
        : {
            changes: { from: range.from, to: range.to },
            range: EditorSelection.cursor(range.from),
          },
    ),
    scrollIntoView: true,
    userEvent: "delete.cut",
  });
  view.focus();
}

export async function pasteIntoSelection(view: EditorView): Promise<void> {
  if (view.state.readOnly) return;
  const text = await readClipboardText();
  // An image-only or empty clipboard inserts nothing rather than clearing the
  // selection, which would silently destroy text.
  if (!text) return;
  // Spread rather than a second spec: `replaceSelection` returns a spec with no
  // scroll field, so merging is the direct way to ask for one.
  view.dispatch({ ...view.state.replaceSelection(text), scrollIntoView: true });
  view.focus();
}
