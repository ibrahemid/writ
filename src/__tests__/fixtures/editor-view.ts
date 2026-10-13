import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { WindowState } from "../../stores/window/createWindowState";

/** Puts `text` in the window's editor as note `id` the way a load does: the view first, then the id. */
export function showInEditor(win: WindowState, id: string, text: string): EditorView {
  const view = new EditorView({ state: EditorState.create({ doc: text }) });
  win.editor.registerView(view);
  win.editor.setCurrentBufferId(id);
  return view;
}

/** Replaces the text in the view the way typing does: the document changes and its readers hear of it. */
export function retypeInEditor(win: WindowState, view: EditorView, text: string): void {
  view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
  win.editor.noteDocChanged();
}
