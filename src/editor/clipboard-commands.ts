import type { EditorView } from "@codemirror/view";
import { readClipboardText, writeClipboardText } from "../services/clipboard";

/**
 * Clipboard verbs for the editor's context menu.
 *
 * Everything goes through CodeMirror transactions rather than
 * `document.execCommand`, so a cut or paste lands in one undo step and a
 * read-only buffer is respected. Reads and writes use the Tauri clipboard
 * plugin (see `services/clipboard`), which behaves the same on all three
 * platforms.
 */

/** Text of the primary selection, or "" when nothing is selected. */
function selectedText(view: EditorView): string {
  const { from, to } = view.state.selection.main;
  return from === to ? "" : view.state.doc.sliceString(from, to);
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
  const { from, to } = view.state.selection.main;
  view.dispatch({
    changes: { from, to, insert: "" },
    selection: { anchor: from },
    scrollIntoView: true,
  });
  view.focus();
}

export async function pasteIntoSelection(view: EditorView): Promise<void> {
  if (view.state.readOnly) return;
  const text = await readClipboardText();
  // An image-only or empty clipboard inserts nothing rather than clearing the
  // selection, which would silently destroy text.
  if (!text) return;
  view.dispatch(
    view.state.replaceSelection(text),
    { scrollIntoView: true },
  );
  view.focus();
}
