import { createSignal } from "solid-js";
import { EditorSelection } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";

export type TransformFn = (input: string) => Promise<string>;

export interface ApplyEditOptions {
  useSelectionIfPresent: boolean;
  transform: TransformFn;
}

export type ApplyEditResult =
  | { applied: true; usedSelection: boolean; outputLength: number }
  | { applied: false; reason: "no-active-view" | "transform-error"; error?: unknown };

export type EditorStore = ReturnType<typeof createEditorStore>;

export function createEditorStore() {
  const [cursorLine, setCursorLine] = createSignal(1);
  const [cursorCol, setCursorCol] = createSignal(1);
  const [lineCount, setLineCount] = createSignal(0);
  const [language, setLanguage] = createSignal<string | null>(null);
  const [selectionCount, setSelectionCount] = createSignal(1);
  // Live text of the active editor view, updated on every document change.
  // The preview pane tracks this and debounces it into a render request.
  const [currentText, setCurrentText] = createSignal("");

  let activeView: EditorView | null = null;

  function registerView(view: EditorView | null) {
    activeView = view;
  }

  function getView(): EditorView | null {
    return activeView;
  }

  function focusEditor() {
    activeView?.focus();
  }

  async function applyEditToActiveBuffer(options: ApplyEditOptions): Promise<ApplyEditResult> {
    const view = activeView;
    if (!view) return { applied: false, reason: "no-active-view" };

    const main = view.state.selection.main;
    const useSelection = options.useSelectionIfPresent && !main.empty;
    const from = useSelection ? main.from : 0;
    const to = useSelection ? main.to : view.state.doc.length;
    const input = view.state.doc.sliceString(from, to);

    let output: string;
    try {
      output = await options.transform(input);
    } catch (error) {
      return { applied: false, reason: "transform-error", error };
    }

    view.dispatch({
      changes: { from, to, insert: output },
      selection: EditorSelection.single(from, from + output.length),
    });
    view.focus();

    return { applied: true, usedSelection: useSelection, outputLength: output.length };
  }

  return {
    cursorLine, setCursorLine,
    cursorCol, setCursorCol,
    lineCount, setLineCount,
    language, setLanguage,
    selectionCount, setSelectionCount,
    currentText, setCurrentText,
    registerView, getView, focusEditor,
    applyEditToActiveBuffer,
  };
}
