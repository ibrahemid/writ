import { createSignal, createRoot } from "solid-js";
import type { EditorView } from "@codemirror/view";

function createEditorStore() {
  const [cursorLine, setCursorLine] = createSignal(1);
  const [cursorCol, setCursorCol] = createSignal(1);
  const [lineCount, setLineCount] = createSignal(0);
  const [language, setLanguage] = createSignal<string | null>(null);
  const [selectionCount, setSelectionCount] = createSignal(1);

  let activeView: EditorView | null = null;

  function registerView(view: EditorView | null) {
    activeView = view;
  }

  function focusEditor() {
    activeView?.focus();
  }

  return {
    cursorLine, setCursorLine,
    cursorCol, setCursorCol,
    lineCount, setLineCount,
    language, setLanguage,
    selectionCount, setSelectionCount,
    registerView, focusEditor,
  };
}

export const editorStore = createRoot(createEditorStore);
