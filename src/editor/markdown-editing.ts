import { Prec, type Extension } from "@codemirror/state";
import { EditorView, keymap, type KeyBinding } from "@codemirror/view";
import {
  toggleBold,
  toggleItalic,
  toggleStrikethrough,
  toggleInlineCode,
  insertLink,
  wrapOnType,
} from "../commands/markdown-format";

export const markdownEditingKeymap: readonly KeyBinding[] = [
  { key: "Mod-b", run: toggleBold },
  { key: "Mod-i", run: toggleItalic },
  { key: "Mod-Shift-x", run: toggleStrikethrough },
  { key: "Mod-e", run: toggleInlineCode },
  { key: "Mod-k", run: insertLink },
];

const markerWrapOnType = EditorView.inputHandler.of((view, _from, _to, text) => {
  const spec = wrapOnType(view.state, text);
  if (!spec) return false;
  view.dispatch(spec);
  return true;
});

export const markdownEditingExtension: Extension = [
  Prec.high(keymap.of([...markdownEditingKeymap])),
  markerWrapOnType,
];
