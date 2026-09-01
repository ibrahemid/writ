import { createSignal } from "solid-js";
import { EditorSelection } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import type { FileOpenMode } from "../../types/buffer";
import {
  debouncedSave,
  cancelAutosave as cancelAutosaveService,
  flushAutosave as flushAutosaveService,
  onAutosaveSuccess,
  saveNow as saveNowService,
  type ContentSource,
  type SaveResult,
} from "../../services/autosave";
import { hashDocument } from "../../lib/doc-hash";
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

const NOTHING_TO_SAVE: SaveResult = { ok: true, failures: [] };

/**
 * How long a note has to stop changing before its document is hashed.
 *
 * Hashing is O(document) and a keystroke is not, so it waits for a pause
 * rather than running on every transaction. Nothing waits on the hash: a
 * document that has changed since its last one already reads dirty
 * ([`isDirty`]).
 */
export const DOC_HASH_IDLE_MS = 150;

/**
 * What is known about one note's text against the file behind it.
 *
 * `docGeneration` counts transactions; `hashedGeneration` records the one
 * `docHash` was taken at. They diverge for as long as an edit has not been
 * hashed, which is what makes an unhashed edit read dirty rather than clean.
 */
interface NoteHashes {
  docGeneration: number;
  hashedGeneration: number;
  docHash?: string;
  diskHash?: string;
}

const FRESH: NoteHashes = { docGeneration: 0, hashedGeneration: 0 };

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

  // Keyed by note, not by the view: a tab in the background has no view and
  // still has to answer whether it differs from its file (U3-U5 read this per
  // tab). A tab switch leaves these entries where they are.
  const [noteHashes, setNoteHashes] = createSignal<ReadonlyMap<string, NoteHashes>>(new Map());
  const hashTimers = new Map<string, ReturnType<typeof setTimeout>>();

  function hashesOf(id: string): NoteHashes {
    return noteHashes().get(id) ?? FRESH;
  }

  function patchHashes(id: string, patch: Partial<NoteHashes>) {
    setNoteHashes((current) => {
      const next = new Map(current);
      next.set(id, { ...(current.get(id) ?? FRESH), ...patch });
      return next;
    });
  }

  function clearHashTimer(id: string) {
    const timer = hashTimers.get(id);
    if (timer) {
      clearTimeout(timer);
      hashTimers.delete(id);
    }
  }

  async function hashNow(id: string, content: ContentSource) {
    const generation = hashesOf(id).docGeneration;
    let text: string;
    try {
      text = typeof content === "function" ? content() : content;
    } catch {
      // The view was torn down between the edit and the timer. The note keeps
      // reading dirty, which is the honest answer for text nobody can measure.
      return;
    }
    const hash = await hashDocument(text);
    // A transaction that arrived while the digest was being computed has
    // already bumped the generation; stamping this one would call the note
    // clean on the strength of text it no longer holds.
    if (hashesOf(id).docGeneration !== generation) return;
    patchHashes(id, { docHash: hash, hashedGeneration: generation });
  }

  /**
   * Records what a note holds the moment it is loaded or reloaded from disk:
   * the document and the file agree, so it is clean.
   */
  function noteOpened(id: string, content: string) {
    clearHashTimer(id);
    setNoteHashes((current) => {
      const next = new Map(current);
      next.set(id, { docGeneration: 0, hashedGeneration: 0 });
      return next;
    });
    void hashDocument(content).then((hash) => {
      if (hashesOf(id).docGeneration !== 0) return;
      patchHashes(id, { docHash: hash, diskHash: hash, hashedGeneration: 0 });
    });
  }

  /**
   * Records that a note's document changed, and hashes it once the edits
   * settle. Until that lands the note reads dirty.
   */
  function noteEdited(id: string, content: ContentSource) {
    patchHashes(id, { docGeneration: hashesOf(id).docGeneration + 1 });
    clearHashTimer(id);
    hashTimers.set(
      id,
      setTimeout(() => {
        hashTimers.delete(id);
        void hashNow(id, content);
      }, DOC_HASH_IDLE_MS),
    );
  }

  /** Records what the note's file holds after a write landed on it. */
  function noteSaved(id: string, diskHash: string | null) {
    if (diskHash === null) return;
    patchHashes(id, { diskHash });
  }

  /** Drops everything held for a note whose tab has gone. */
  function noteClosed(id: string) {
    clearHashTimer(id);
    setNoteHashes((current) => {
      if (!current.has(id)) return current;
      const next = new Map(current);
      next.delete(id);
      return next;
    });
  }

  /**
   * Whether the note's document differs from the file behind it.
   *
   * The autosave queue is never consulted: a write that just resolved empties
   * it while the document has moved on again, and a note with nothing queued
   * can still hold text no file has.
   */
  function isDirty(id: string): boolean {
    const state = noteHashes().get(id);
    if (state === undefined) return false;
    if (state.docGeneration !== state.hashedGeneration) return true;
    return state.docHash !== state.diskHash;
  }

  function docHash(id: string): string | undefined {
    return noteHashes().get(id)?.docHash;
  }

  function lastKnownDiskHash(id: string): string | undefined {
    return noteHashes().get(id)?.diskHash;
  }

  // A landed write is the one thing that moves the record of the file without
  // the editor doing anything, so the store listens for it rather than making
  // every save path remember to report back.
  const stopSaveListener = onAutosaveSuccess(noteSaved);

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

  function flushAutosave(bufferId?: string): Promise<SaveResult> {
    return flushAutosaveService(bufferId);
  }

  // The explicit save. Writes the live document of the loaded buffer even when
  // no edit is pending, so the keystroke always means "it is on disk now". A
  // binary buffer is skipped: it opens read-only and its view holds a decoded
  // rendering, never the bytes to write back.
  function saveActiveBuffer(): Promise<SaveResult> {
    const bufferId = currentBufferId();
    const view = activeView;
    if (bufferId === null || view === null) return Promise.resolve(NOTHING_TO_SAVE);
    if (largeFileMode()?.kind === "Binary") return Promise.resolve(NOTHING_TO_SAVE);
    return saveNowService(bufferId, () => view.state.doc.toString());
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
    scheduleAutosave, cancelAutosave, flushAutosave, saveActiveBuffer,
    detectLanguage, detectFromContent,
    noteOpened, noteEdited, noteSaved, noteClosed,
    isDirty, docHash, lastKnownDiskHash,
    stopSaveListener,
  };
}
