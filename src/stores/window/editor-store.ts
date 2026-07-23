import { createSignal } from "solid-js";
import { EditorSelection } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import type { FileOpenMode } from "../../types/buffer";
import {
  debouncedSave,
  cancelAutosave as cancelAutosaveService,
  flushAutosave as flushAutosaveService,
  type ContentSource,
} from "../../services/autosave";
import {
  detectLanguage as detectLanguageService,
  detectFromContent as detectFromContentService,
} from "../../services/language-detect";

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

  // A request to move the cursor to a line and scroll it into view, raised when
  // a search result is opened. EditorInstance consumes it once the matching
  // buffer is loaded (gating on currentBufferId), so a reveal fired before an
  // async tab switch finishes still lands on the right line. The seq makes
  // repeated reveals of the same buffer/line each fire.
  const [pendingReveal, setPendingReveal] =
    createSignal<{ bufferId: string; line: number; seq: number } | null>(null);
  let revealSeq = 0;

  function requestReveal(bufferId: string, line: number) {
    revealSeq += 1;
    setPendingReveal({ bufferId, line, seq: revealSeq });
  }

  // Cleared by EditorInstance once a reveal has been applied, so a later
  // republish of currentBufferId for an already-loaded buffer can never re-yank
  // the cursor to a stale search line.
  function clearReveal() {
    setPendingReveal(null);
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

  // Reads the range a rewrite would act on: the selection when one is present,
  // otherwise the whole document. Returns the char offsets so the range can be
  // anchored and mapped through later edits.
  function getSelectionRange(
    useSelectionIfPresent: boolean,
  ): { from: number; to: number; text: string; usedSelection: boolean } | null {
    const view = activeView;
    if (!view) return null;
    const main = view.state.selection.main;
    const usedSelection = useSelectionIfPresent && !main.empty;
    const from = usedSelection ? main.from : 0;
    const to = usedSelection ? main.to : view.state.doc.length;
    return { from, to, text: view.state.doc.sliceString(from, to), usedSelection };
  }

  // Replaces an anchored range in a single dispatch (one undo step), selects
  // the inserted text, and refocuses. Offsets are clamped so a stale anchor can
  // never dispatch out of bounds.
  function replaceRange(from: number, to: number, insert: string): boolean {
    const view = activeView;
    if (!view) return false;
    const docLen = view.state.doc.length;
    const clampedFrom = Math.max(0, Math.min(from, docLen));
    const clampedTo = Math.max(clampedFrom, Math.min(to, docLen));
    view.dispatch({
      changes: { from: clampedFrom, to: clampedTo, insert },
      selection: EditorSelection.single(clampedFrom, clampedFrom + insert.length),
    });
    view.focus();
    return true;
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

  // Autosave and language detection are services; the editor component routes
  // through these so it only ever talks to its store (layering rule).
  function scheduleAutosave(bufferId: string, content: ContentSource, delayMs: number) {
    debouncedSave(bufferId, content, delayMs);
  }

  function cancelAutosave(bufferId: string) {
    cancelAutosaveService(bufferId);
  }

  function flushAutosave(bufferId?: string): Promise<void> {
    return flushAutosaveService(bufferId);
  }

  function detectLanguage(content: string, filename?: string): string | null {
    return detectLanguageService(content, filename);
  }

  function detectFromContent(content: string): string | null {
    return detectFromContentService(content);
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
    pendingReveal, requestReveal, clearReveal,
    largeFileMode, setLargeFileMode,
    registerView, getView, focusEditor,
    getActiveText,
    getSelectionRange,
    replaceRange,
    applyEditToActiveBuffer,
    scheduleAutosave, cancelAutosave, flushAutosave,
    detectLanguage, detectFromContent,
  };
}
