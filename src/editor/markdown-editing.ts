import { type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { wrapOnType } from "../commands/markdown-format";

// The formatting chords (bold/italic/strikethrough/inline code/link) are owned
// by the command registry (see EditorInstance's format-command effect), so they
// are not bound here — a second CM keymap would ghost a user's rebind.
const markerWrapOnType = EditorView.inputHandler.of((view, _from, _to, text) => {
  if (view.composing) return false;
  const spec = wrapOnType(view.state, text);
  if (!spec) return false;
  view.dispatch(spec);
  return true;
});

export const markdownEditingExtension: Extension = [markerWrapOnType];
