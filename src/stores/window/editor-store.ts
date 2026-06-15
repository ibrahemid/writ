import { createSignal } from "solid-js";
import { EditorSelection } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import type { FileOpenMode } from "../../types/buffer";

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
  const [largeFileMode, setLargeFileMode] = createSignal<FileOpenMode | null>(null);
  // Live text of the active editor view, updated on every document change.
  // The preview pane tracks this and debounces it into a render request.
  const [currentText, setCurrentText] = createSignal("");
  // Id of the buffer whose content is currently loaded into the active view.
  // Published by EditorInstance.loadBuffer only after the buffer's content is
  // read in, so it stays consistent with currentText. The preview pane gates
  // rendering on this matching its own buffer id: during a tab switch
  // props.buffer.id flips reactively while the editor is still mid-load on the
  // outgoing buffer, and rendering then would cache the wrong buffer's HTML
  // under the incoming id (the #97 stale-cache flash).
  const [currentBufferId, setCurrentBufferId] = createSignal<string | null>(null);
  // A monotonically-keyed request to reload the active buffer's content from
  // disk, raised when the file changed externally (audit blocker #53.4).
  // EditorInstance consumes it; the seq makes repeated external edits to the
  // same buffer each fire a fresh reload.
  const [externalReload, setExternalReload] =
    createSignal<{ id: string; seq: number } | null>(null);
  let reloadSeq = 0;

  function requestExternalReload(bufferId: string) {
    reloadSeq += 1;
    setExternalReload({ id: bufferId, seq: reloadSeq });
  }

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

  function getActiveText(
    useSelectionIfPresent: boolean,
  ): { text: string; usedSelection: boolean } | null {
    const view = activeView;
    if (!view) return null;
    const main = view.state.selection.main;
    const useSelection = useSelectionIfPresent && !main.empty;
    const from = useSelection ? main.from : 0;
    const to = useSelection ? main.to : view.state.doc.length;
    return { text: view.state.doc.sliceString(from, to), usedSelection: useSelection };
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
    currentBufferId, setCurrentBufferId,
    externalReload, requestExternalReload,
    largeFileMode, setLargeFileMode,
    registerView, getView, focusEditor,
    getActiveText,
    applyEditToActiveBuffer,
  };
}
