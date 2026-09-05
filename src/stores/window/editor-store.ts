import { createSignal } from "solid-js";
import { EditorSelection } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import type { FileOpenMode } from "../../types/buffer";
import { NO_ACTIVE_FORMATS, type ActiveFormats } from "../../types/editor";
import {
  debouncedSave,
  cancelAutosave as cancelAutosaveService,
  flushAutosave as flushAutosaveService,
  holdUnsavedContent,
  peekHeldContent,
  onAutosaveSuccess,
  peekUnsavedContent,
  releaseUnsavedContent,
  saveNow as saveNowService,
  type ContentSource,
  type SaveResult,
} from "../../services/autosave";
import { noteDiskState, restoreNoteFile, type NoteDiskAnswer } from "../../services/tauri";
import { applyExternalDocument } from "../../editor/external-reload";
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
  | {
      applied: false;
      reason: "no-active-view" | "transform-error";
      error?: unknown;
    };

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
 * How long `Updated from disk` stays in the status region.
 *
 * Long enough to read on the way back from another window, short enough that
 * it is gone before it starts reading as the note's state rather than as
 * something that happened.
 */
export const UPDATED_FROM_DISK_MS = 4000;

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
  /**
   * Which call to [`noteOpened`] this record belongs to. Counted per store and
   * never reset, so a call that a later one has overtaken can tell: the
   * record's number is no longer its own, and the answer it is carrying
   * describes a version of the note the tab has already moved off.
   * `docGeneration` cannot say this, because `noteOpened` sets that back to 0
   * on every call and two calls for one note both see 0.
   */
  openTicket?: number;
  /**
   * Whether that call is still out. A record installed and not yet filled
   * knows nothing about either side, which is the same "no idea" as no record
   * at all, and [`isDirty`] and [`isTracked`] answer it the same way.
   */
  openInFlight?: boolean;
  /**
   * Whether a write landed while that call was out. The write's digest is of
   * what it just put on the file and the open's is of a read that happened
   * before it, so the fill leaves `diskHash` alone when this is set. Cleared
   * with the flight, and by the next call, which replaces the record whole.
   */
  writtenDuringOpen?: boolean;
  docHash?: string;
  diskHash?: string;
}

const FRESH: NoteHashes = { docGeneration: 0, hashedGeneration: 0 };

/**
 * What the note's file is doing, as far as the tab knows.
 *
 * One state per note rather than one flag per bar. Independent flags let a
 * note hold "the file changed" and "the file is gone" at the same time, which
 * puts two bars on screen and offers an answer against a file that is not
 * there: `Keep mine` reads the file it is answering about, and a file that was
 * deleted cannot be read. There is one file behind a note, so there is one
 * state.
 *
 * `present` is the ordinary state and is held by absence from the map.
 */
export type NoteFileState = "present" | "changed" | "removed";

/**
 * What reached the tab about its file: the three changes the watcher reports
 * (`buffer:external`), and the two ways a hold ends, which are a write of the
 * tab's text landing and the note and its file being seen to agree.
 */
export type NoteFileEvent = "modified" | "removed" | "moved" | "written" | "settled";

/**
 * The whole transition table, in one place.
 *
 * - `modified` asks, whatever the note held before. A file recreated with
 *   different bytes after it was deleted is a question, not a deletion.
 * - `removed` outranks everything. A file that is gone has no text to offer,
 *   so the question about its text goes and the removed bar's answers are the
 *   only ones left (ADR-033 §15).
 * - `moved` changes no bytes: it clears a deletion, because the file turned
 *   out to have gone somewhere rather than nowhere, and keeps a question,
 *   because the file at the new path still differs from the tab. The bar
 *   answers through the note's id and the command re-reads the note's current
 *   path (`src-tauri/src/commands/buffer.rs` `resolve_external_change_inner`),
 *   so the answer lands on the file where it now is.
 * - `written` ends a question and nothing else. A write that landed says there
 *   was a file when it left, not that there is one now: a save already in
 *   flight when the deletion arrived would otherwise drop the removed bar for
 *   a file that is still gone.
 * - `settled` ends both, and is only ever said by something that has just
 *   seen the file: the tab took the file's own text, or the write-back put the
 *   file at the note's path again. A deletion otherwise ends by the file
 *   coming back (`moved`, `modified`), by a copy written as a new note, or
 *   with the tab.
 */
function nextNoteFileState(
  current: NoteFileState,
  event: NoteFileEvent,
): NoteFileState {
  switch (event) {
    case "modified":
      return "changed";
    case "removed":
      return "removed";
    case "moved":
      return current === "removed" ? "present" : current;
    case "written":
      return current === "changed" ? "present" : current;
    case "settled":
      return "present";
  }
}

export function createEditorStore() {
  const [cursorLine, setCursorLine] = createSignal(1);
  const [cursorCol, setCursorCol] = createSignal(1);
  const [lineCount, setLineCount] = createSignal(0);
  const [language, setLanguage] = createSignal<string | null>(null);
  const [selectionCount, setSelectionCount] = createSignal(1);
  // Which markdown constructs the caret sits inside, republished by the active
  // view on every selection or document change. The toolbar reads it for the
  // pressed state of its formatting controls.
  const [activeFormats, setActiveFormats] = createSignal<ActiveFormats>(NO_ACTIVE_FORMATS);
  const [largeFileMode, setLargeFileMode] = createSignal<FileOpenMode | null>(
    null,
  );
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
  const [currentBufferId, setCurrentBufferId] = createSignal<string | null>(
    null,
  );
  // A monotonically-keyed request to reload the active buffer's content from
  // disk, raised when the file changed externally (audit blocker #53.4).
  // EditorInstance consumes it; the seq makes repeated external edits to the
  // same buffer each fire a fresh reload.
  const [externalReload, setExternalReload] = createSignal<{
    id: string;
    seq: number;
  } | null>(null);
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
  const [pendingReveal, setPendingReveal] = createSignal<{
    bufferId: string;
    line: number;
    seq: number;
  } | null>(null);
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
  const [noteHashes, setNoteHashes] = createSignal<
    ReadonlyMap<string, NoteHashes>
  >(new Map());
  const hashTimers = new Map<string, ReturnType<typeof setTimeout>>();
  // Handed out by `noteOpened`, one per call, and never reset. A counter that
  // started again would let a call read its own number off a record a later
  // call happens to have stamped with the same one.
  let openTickets = 0;

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

  // What each note's file is doing. A note with nothing in it here is
  // `present`: its file is where the tab left it and holds what the tab was
  // last told it holds.
  //
  // `changed` is a question nobody has answered yet. Nothing is replaced and
  // nothing is written until they do: this is what the bar reads, and what
  // stops the answer being settled for them by whichever write landed last
  // (spec W5).
  //
  // `removed` is a file that is gone while its tab stayed open. The text is
  // still in the editor and still the only copy of it, so the tab keeps it and
  // writes nothing: recreating the file would put back what the person threw
  // away, and in a synced folder it would put it back on every device (W4).
  // The backend refuses such a save under ERR_FILE_REMOVED_ON_DISK whatever
  // this holds; this is what stops the tab asking in the first place.
  const [noteFileStates, setNoteFileStates] = createSignal<
    ReadonlyMap<string, NoteFileState>
  >(new Map());

  // The text of a note whose file is gone, which is the last copy of it. It is
  // kept here and not in the editor view because a tab switch destroys the
  // view, and there is no file left to read the text back from: a note that
  // lost its file and then its view has lost the note.
  const [removedText, setRemovedText] = createSignal<ReadonlyMap<string, string>>(
    new Map(),
  );

  function noteFileState(id: string): NoteFileState {
    return noteFileStates().get(id) ?? "present";
  }

  /**
   * The one way a note's file state moves. Table: [`nextNoteFileState`].
   *
   * The order inside a move into a hold is the whole point. The text is read
   * first: the tab on screen holds it in its view, and a background tab holds
   * it in the autosave service, either queued or left there by a write that
   * came back refused. The queue is cancelled second, because cancelling drops
   * both of those, and for a background tab whose save failed that was the
   * only copy of it there is. What was read is then held beside the queue.
   *
   * A report that changes nothing returns at once, so a second deletion cannot
   * cancel a queue the first one has since put text back into.
   */
  function recordFileEvent(id: string, event: NoteFileEvent) {
    const before = noteFileState(id);
    const after = nextNoteFileState(before, event);
    if (after === before) return;

    // Materialized only where it is wanted. This runs on every landed write,
    // and reading a large document there would put the cost ADR-020 keeps off
    // the keystroke back onto the save.
    const wantsText = after !== "present" || event === "moved";
    const kept = wantsText ? (liveTextOfNote(id) ?? peekUnsavedContent(id)) : undefined;

    // The note may write again, and every way a hold ends has already dealt
    // with the text: two answers write it, the third replaces it on purpose, a
    // quiet reload only happens to a note with nothing of its own, and a file
    // found at another path is handled below. Held any longer it would come
    // back as a recovered note holding the version the person chose against.
    if (after === "present") {
      releaseUnsavedContent(id);
      // And the copy the next load of the tab would read in place of the file.
      // There is a file again, and it is the one the tab is about to hold.
      setRemovedText((current) => {
        if (!current.has(id)) return current;
        const next = new Map(current);
        next.delete(id);
        return next;
      });
    }

    setNoteFileStates((current) => {
      const next = new Map(current);
      if (after === "present") next.delete(id);
      else next.set(id, after);
      return next;
    });

    if (after !== "present") {
      // The queued write was aimed at the file this hold is about. Letting it
      // run would answer for the person, and the guard would refuse it and
      // stack a second bar under the first.
      cancelAutosaveService(id);
      if (kept !== undefined) holdText(id, kept);
      return;
    }

    // A file found at another path is the one end that neither writes the
    // tab's text nor replaces it. The note is writable again at its new path,
    // and what it has been holding since the deletion is the only copy of it:
    // the queue went when the hold went on, so nothing else would write it.
    if (event === "moved" && kept !== undefined && isDirty(id)) {
      debouncedSave(id, kept, 0);
    }
  }

  /** The live document of `id`, when it is the note the editor is showing. */
  function liveTextOfNote(id: string): string | undefined {
    if (currentBufferId() === id && activeView !== null) {
      return activeView.state.doc.toString();
    }
    return undefined;
  }

  /**
   * Keeps `text` where a close, a quit and the next load can find it.
   *
   * Two readers. The recovery handover reads the autosave service's hold; the
   * next load of the tab reads [`removedText`], because the view is destroyed
   * on a switch and a note that may not write has nothing on disk to be read
   * back from. A `removed` note has no file at all; a `changed` one has a file
   * holding somebody else's text, which is the version the question exists to
   * keep out of the tab until it is answered. So both are kept here, and the
   * copy is refreshed on every hold rather than written once: after
   * `removed -> changed` the entry is whatever the note holds now, and it is
   * dropped only when the note reaches `present` ([`recordFileEvent`]).
   */
  function holdText(id: string, text: string) {
    holdUnsavedContent(id, text);
    if (!savesAreHeld(id)) return;
    setRemovedText((current) => {
      if (current.get(id) === text) return current;
      const next = new Map(current);
      next.set(id, text);
      return next;
    });
  }

  /**
   * Records that a note's file is gone, keeping whatever text only Writ holds.
   *
   * `text` is passed only by the launch, which seeds a note the last session
   * left removed from the shutdown snapshot rather than from anything live.
   * Everything else is [`recordFileEvent`]'s, including the read-then-cancel
   * order and the early return for a note already marked.
   */
  function markRemovedOnDisk(id: string, text?: string) {
    if (isRemovedOnDisk(id)) return;
    recordFileEvent(id, "removed");
    if (text !== undefined) keepTextOfRemoved(id, text);
  }

  /**
   * The note's file is at its own path again, holding what the tab holds.
   *
   * The write-back is the one write that says so. An ordinary save that landed
   * says only that there was a file when it left: it may have been in flight
   * when the deletion arrived, and dropping the mark on it would take the bar
   * off a file that is still gone ([`nextNoteFileState`]).
   */
  function clearRemovedOnDisk(id: string) {
    recordFileEvent(id, "settled");
  }

  // The state and the text, dropped without saying anything about the file. A
  // closed tab has no file either way, and its text has not reached the
  // snapshot yet: `keepUnsavedForRecovery` reads it after this and cancels the
  // note, which is what takes it back out.
  function forgetFileState(id: string) {
    setNoteFileStates((current) => {
      if (!current.has(id)) return current;
      const next = new Map(current);
      next.delete(id);
      return next;
    });
    setRemovedText((current) => {
      if (!current.has(id)) return current;
      const next = new Map(current);
      next.delete(id);
      return next;
    });
  }

  function isRemovedOnDisk(id: string): boolean {
    return noteFileState(id) === "removed";
  }

  function isFileChangedOnDisk(id: string): boolean {
    return noteFileState(id) === "changed";
  }

  /**
   * Whether the tab may write to its file at all.
   *
   * `removed` holds writes because there is no file to write to and
   * recreating one puts back what the person threw away (W4). `changed` holds
   * them because the bar is an unanswered question about that same file: the
   * guard refuses every write while the file differs, and each refusal leaves
   * another dated copy beside the note (`BufferStore::write_conflict_copy`),
   * so ordinary typing would fill the notes folder with copies while the
   * question is still on screen. The bar's three answers are the only way the
   * tab's text reaches disk from here.
   *
   * Held, not dropped. The text stays in the document, it is kept for the
   * recovery snapshot so closing the tab or quitting does not lose it
   * (`holdUnsavedContent`), and the note saves normally again the moment its
   * state is `present`.
   *
   * A `removed` note's text is kept the same way, and the deletion still
   * stands: the next launch writes nothing at the path and leaves nothing
   * beside it either, handing the text to the tab, which comes up removed on
   * disk (`writ_core::recovery::plan_recovery`). So the typing survives a quit
   * and the file the person threw away is not put back.
   *
   * The bar of a failed save reads this too, and takes its `Try again` away
   * while it holds: a save already in flight when the watcher reported can
   * fail under the question with a retryable reason, and the button under it
   * would reach the same hold and do nothing.
   */
  function savesAreHeld(id: string): boolean {
    return noteFileState(id) !== "present";
  }

  // A request for the file-changed bar to take the focus, raised when a save
  // is asked for while its question is still up. Carried as state rather than
  // reached for in the DOM, and sequenced so a second press moves the focus
  // again.
  const [pendingChangeAnswer, setPendingChangeAnswer] = createSignal<{
    id: string;
    seq: number;
  } | null>(null);
  let changeAnswerSeq = 0;

  function askForChangeAnswer(id: string) {
    changeAnswerSeq += 1;
    setPendingChangeAnswer({ id, seq: changeAnswerSeq });
  }

  // The note whose text its file last replaced, cleared on a timer. One at a
  // time: the marker reports on the tab in front, and it says what just
  // happened rather than what is true, so it goes away on its own.
  const [updatedFromDisk, setUpdatedFromDisk] = createSignal<string | null>(
    null,
  );
  let updatedFromDiskTimer: ReturnType<typeof setTimeout> | undefined;

  function markUpdatedFromDisk(id: string) {
    if (updatedFromDiskTimer !== undefined) clearTimeout(updatedFromDiskTimer);
    setUpdatedFromDisk(id);
    updatedFromDiskTimer = setTimeout(() => {
      updatedFromDiskTimer = undefined;
      setUpdatedFromDisk(null);
    }, UPDATED_FROM_DISK_MS);
  }

  function clearUpdatedFromDisk(id: string) {
    if (updatedFromDisk() !== id) return;
    if (updatedFromDiskTimer !== undefined) clearTimeout(updatedFromDiskTimer);
    updatedFromDiskTimer = undefined;
    setUpdatedFromDisk(null);
  }

  function isUpdatedFromDisk(id: string): boolean {
    return updatedFromDisk() === id;
  }

  /**
   * Puts text the file holds into the note, wherever the note is.
   *
   * The one way external text reaches a document: the quiet reload of a note
   * with nothing to lose and the answer to the bar both come through here, so
   * the tracked transaction, the record of what the file holds and the marker
   * happen once each and in one place.
   *
   * A note that is not the one in the view is recorded rather than dispatched.
   * It has no view to dispatch into, and the tab reads its file again when it
   * is switched to; dropping the record instead would leave the note reading
   * dirty against a file it matches.
   */
  function applyExternalContent(id: string, text: string) {
    const view = activeView;
    if (view && currentBufferId() === id) {
      applyExternalDocument(view, text);
      setCurrentText(text);
      setLineCount(view.state.doc.lines);
    }
    noteOpened(id, text);
    // The note now holds what its file holds, so whatever was being asked
    // about the difference between them is answered.
    recordFileEvent(id, "settled");
    markUpdatedFromDisk(id);
  }

  /**
   * Keeps `text` as the last copy of a note that may not write.
   *
   * The caller hands the text over rather than this reading the live view: the
   * mark arrives for a background tab as well, and the view is then holding a
   * different note's document.
   *
   * `savesAreHeld` and not `isRemovedOnDisk`: the tab being switched away from
   * is the moment a `changed` note's text would otherwise go with the view,
   * and its file holds the version the question is about rather than this one.
   */
  function keepTextOfRemoved(id: string, text: string) {
    if (!savesAreHeld(id)) return;
    // The same text the next load reads is the text a close or a quit has to
    // keep, and no write is going to leave it there in the usual way.
    holdText(id, text);
  }

  /**
   * The newest text of `id` that no file is known to hold.
   *
   * The live document when the note is the one on screen, else what its hold
   * is keeping. Never a failed write's text: the caller asking this is about
   * to decide what to write, and a refusal that a later answer superseded is
   * not it ([`peekHeldContent`]).
   */
  function liveTextOf(id: string): string | undefined {
    return liveTextOfNote(id) ?? peekHeldContent(id);
  }

  /** The kept text of a note that may not write, when there is one. */
  function textOfRemoved(id: string): string | undefined {
    return removedText().get(id);
  }

  /** The newest text of a removed note: the live document, else what is kept. */
  function liveTextOfRemoved(id: string): string | undefined {
    return liveTextOfNote(id) ?? textOfRemoved(id);
  }

  /**
   * Writes a note whose file was deleted outside Writ back to the path it
   * names.
   *
   * The write goes through the one command the backend does not refuse for a
   * removed file, and the mark is dropped only once the write lands: a restore
   * that failed leaves the note removed, so no later keystroke recreates the
   * file behind the person's back. A failure reports through the same
   * listeners every other save does, so it reaches the same bar.
   */
  async function restoreRemovedFile(id: string): Promise<SaveResult> {
    const text = liveTextOfRemoved(id);
    if (text === undefined) return NOTHING_TO_SAVE;
    const result = await saveNowService(id, text, restoreNoteFile);
    if (result.ok) clearRemovedOnDisk(id);
    return result;
  }

  function forgetHashes(id: string) {
    setNoteHashes((current) => {
      if (!current.has(id)) return current;
      const next = new Map(current);
      next.delete(id);
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
   * Records what a note holds the moment it is loaded or reloaded from disk.
   *
   * Two digests, from the two sides they describe: the document's is computed
   * here, the file's comes from Rust, which read the file
   * (`writ_core::hash::comparison_digest_hex`). The frontend never computes a
   * digest for a file. It could compute one that agrees, since both sides
   * normalise line endings the same way, but then the number the dirty
   * predicate rests on would have two authors and only one of them would ever
   * see the file change under it.
   *
   * A note with no file yet records neither digest. The two sides then agree
   * that nothing is known to differ, which is right for a new empty note; the
   * first keystroke moves the generation and the note reads dirty from there
   * until a save gives it a file.
   *
   * A note whose file could not be described — not there, or its bytes not on
   * this machine — drops its record instead, the same as the call failing
   * outright. Either way the editor holds a document with nothing to compare
   * it against, and `isDirty` answers dirty for a note it holds no record of,
   * which is what stops a later reload replacing text no file holds.
   */
  function noteOpened(id: string, content: string) {
    clearHashTimer(id);
    const ticket = ++openTickets;
    setNoteHashes((current) => {
      const next = new Map(current);
      next.set(id, {
        docGeneration: 0,
        hashedGeneration: 0,
        openTicket: ticket,
        openInFlight: true,
      });
      return next;
    });
    void (async () => {
      // Every exit below is guarded, not only the one that fills the record.
      // A superseded call that dropped its record instead would take the
      // live one with it, which is the same overwrite by a different route.
      let answer: NoteDiskAnswer;
      try {
        answer = await noteDiskState(id);
      } catch {
        if (supersededOpen(id, ticket)) return;
        forgetHashes(id);
        return;
      }
      if (supersededOpen(id, ticket)) return;
      if (answer.state === "undescribed") {
        forgetHashes(id);
        return;
      }
      if (answer.state === "no_file") {
        // A note the tab is holding text for is a different question. Reading
        // clean off two empty digests would let the file's return be reloaded
        // over that text, so it answers the way an undescribed file does and
        // the person is asked. The backend reaches this only for a note that
        // names no file at all, and this does not depend on that.
        if (savesAreHeld(id)) {
          forgetHashes(id);
          return;
        }
        // Nothing to compare against and nothing more coming: the note reads
        // untracked from here rather than in flight forever.
        patchHashes(id, { openInFlight: false, writtenDuringOpen: false });
        return;
      }
      const documentHash = await hashDocument(content);
      if (supersededOpen(id, ticket)) return;
      // An edit that landed while either answer was in flight has moved the
      // generation, and this pair describes a document that is already gone.
      if (hashesOf(id).docGeneration !== 0) {
        patchHashes(id, { openInFlight: false, writtenDuringOpen: false });
        return;
      }
      // A write that landed since this read went out knows the file better
      // than this read does, and it already recorded what it wrote.
      const wroteSince = hashesOf(id).writtenDuringOpen === true;
      patchHashes(id, {
        docHash: documentHash,
        ...(wroteSince ? {} : { diskHash: answer.disk.hash }),
        hashedGeneration: 0,
        openInFlight: false,
        writtenDuringOpen: false,
      });
    })();
  }

  /** Whether a later [`noteOpened`] has taken the note off this call's answer. */
  function supersededOpen(id: string, ticket: number): boolean {
    return hashesOf(id).openTicket !== ticket;
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

  /**
   * Records what the note's file holds after a write landed on it.
   *
   * `diskHash` is the digest the save command computed over what it wrote, so
   * the file's side of the comparison comes from Rust here too. Null when the
   * note had nothing in it and no file to mint one for.
   *
   * `viaWriteBack` separates the two writes that can land here. The write-back
   * put the file at the note's path again, so the removed mark comes off here
   * rather than only where the restore was asked for: a restore requeued after
   * a failure lands from the autosave queue, and the bar would otherwise stay
   * over a file that is back. An ordinary save says only that there was a file
   * when it left ([`nextNoteFileState`]).
   */
  function noteSaved(id: string, diskHash: string | null, viaWriteBack: boolean) {
    recordFileEvent(id, viaWriteBack ? "settled" : "written");
    if (diskHash === null) return;
    // An open that is still out read the file before this write changed it,
    // and its answer would otherwise land on top of this one.
    const duringOpen = hashesOf(id).openInFlight === true;
    patchHashes(id, duringOpen ? { diskHash, writtenDuringOpen: true } : { diskHash });
  }

  /** Drops everything held for a note whose tab has gone. */
  function noteClosed(id: string) {
    clearHashTimer(id);
    forgetHashes(id);
    forgetFileState(id);
    clearUpdatedFromDisk(id);
  }

  /**
   * Whether the note's document differs from the file behind it.
   *
   * The autosave queue is never consulted: a write that just resolved empties
   * it while the document has moved on again, and a note with nothing queued
   * can still hold text no file has.
   *
   * Fail closed. A note with no record — a tab restored at launch that has
   * never been opened, one whose record was dropped — answers `true`, because
   * the callers of this are deciding whether a file may be reloaded over the
   * document, and "no idea" has to stop that. Ask [`isTracked`] first when the
   * question is whether there is anything to tell the person about.
   *
   * A record whose open is still out is the same "no idea" and answers the
   * same way. Its two digests are both `undefined`, so comparing them reads
   * clean, and a note the tab is still opening would let a change arriving in
   * that window reload the file over text the tab is holding and no file has.
   */
  function isDirty(id: string): boolean {
    const state = noteHashes().get(id);
    if (state === undefined) return true;
    if (state.openInFlight === true) return true;
    if (state.docGeneration !== state.hashedGeneration) return true;
    return state.docHash !== state.diskHash;
  }

  /**
   * Whether the store holds a record of what this note and its file hold.
   *
   * A record whose open has not answered yet holds neither, so it counts as
   * no record: this is what a caller asks before [`isDirty`] to keep the
   * fail-closed answer out of anything shown to the person.
   */
  function isTracked(id: string): boolean {
    const state = noteHashes().get(id);
    return state !== undefined && state.openInFlight !== true;
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
    return {
      text: view.state.doc.sliceString(from, to),
      usedSelection: useSelection,
    };
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
    return {
      from,
      to,
      text: view.state.doc.sliceString(from, to),
      usedSelection,
    };
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
      selection: EditorSelection.single(
        clampedFrom,
        clampedFrom + insert.length,
      ),
    });
    view.focus();
    return true;
  }

  async function applyEditToActiveBuffer(
    options: ApplyEditOptions,
  ): Promise<ApplyEditResult> {
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

    return {
      applied: true,
      usedSelection: useSelection,
      outputLength: output.length,
    };
  }

  // Autosave and language detection are services; the editor component routes
  // through these so it only ever talks to its store (layering rule).
  function scheduleAutosave(
    bufferId: string,
    content: ContentSource,
    delayMs: number,
  ) {
    // A note carrying a bar writes nothing until the bar is answered, and a
    // note whose file is gone writes nothing at all. Without this every
    // keystroke queues a save the backend refuses, the bar's reason is
    // replaced by a fresh failure each time, and a file that changed leaves a
    // dated copy beside the note for every pause in typing. The queue is empty
    // while the bar is up, so the flushes on quit, blur and tab switch find
    // nothing to write either. Reasons: [`savesAreHeld`].
    //
    // The keystroke is still the newest text there is of that note, so it is
    // kept rather than dropped: it goes to the slot the recovery handover
    // reads and no write path does, and for a note whose file is gone to the
    // copy the next load of its tab reads ([`holdText`]).
    //
    // Materialized here rather than held as a getter. A getter reads the
    // editor's single view, and the next tab switch destroys that view and
    // builds another note's in its place, so a held getter would report the
    // incoming note's document under this one's name.
    if (savesAreHeld(bufferId)) {
      holdText(bufferId, typeof content === "function" ? content() : content);
      return;
    }
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
  //
  // A note whose file was deleted goes back to that path instead. Autosave
  // stays silent for it, but the save keystroke is the person asking, and
  // answering it with nothing at all would be the tab keeping quiet about the
  // one thing it is showing a bar for.
  function saveActiveBuffer(): Promise<SaveResult> {
    const bufferId = currentBufferId();
    const view = activeView;
    if (bufferId === null || view === null)
      return Promise.resolve(NOTHING_TO_SAVE);
    // The bar is what "save it" means for a note whose file changed: it is
    // asking which text the file ends up with, and all three of its answers
    // write. So the keystroke goes to the question rather than past it.
    // Nothing is written and nothing is said, because the answer is already on
    // screen. A note whose file is gone is the other case, below: there is no
    // question, and the keystroke is the person asking for the file back.
    if (isFileChangedOnDisk(bufferId)) {
      askForChangeAnswer(bufferId);
      return Promise.resolve(NOTHING_TO_SAVE);
    }
    if (largeFileMode()?.kind === "Binary")
      return Promise.resolve(NOTHING_TO_SAVE);
    if (isRemovedOnDisk(bufferId)) return restoreRemovedFile(bufferId);
    return saveNowService(bufferId, () => view.state.doc.toString());
  }

  /**
   * Writes the note's outstanding text again, after a failure.
   *
   * The text comes from autosave rather than the view: a write the guard
   * stopped leaves the queue empty on purpose, and the note whose bar is on
   * screen is not always the one loaded into the editor.
   *
   * A note whose file was deleted outside Writ is put back instead. Writing it
   * the ordinary way is refused every time, so the press would answer the bar
   * with the same bar.
   *
   * A note under a question of its own writes nothing. A save already in
   * flight when the watcher reported keeps the text it could not write, so
   * this can still be reached under the bar, and it would write into the same
   * refusal the question is about.
   */
  function retrySave(id: string): Promise<SaveResult> {
    if (isRemovedOnDisk(id)) return restoreRemovedFile(id);
    if (savesAreHeld(id)) return Promise.resolve(NOTHING_TO_SAVE);
    const content = peekUnsavedContent(id);
    if (content === undefined) return Promise.resolve(NOTHING_TO_SAVE);
    return saveNowService(id, content);
  }

  /** What the backend can say about the note's file right now. */
  function readDiskState(id: string): Promise<NoteDiskAnswer> {
    return noteDiskState(id);
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
    activeFormats, setActiveFormats,
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
    scheduleAutosave,
    cancelAutosave,
    flushAutosave,
    saveActiveBuffer,
    savesAreHeld,
    retrySave,
    readDiskState,
    detectLanguage,
    detectFromContent,
    noteOpened,
    noteEdited,
    noteSaved,
    noteClosed,
    isDirty,
    isTracked,
    noteFileStates,
    noteFileState,
    recordFileEvent,
    pendingChangeAnswer,
    isRemovedOnDisk,
    isFileChangedOnDisk,
    markRemovedOnDisk,
    clearRemovedOnDisk,
    keepTextOfRemoved,
    textOfRemoved,
    liveTextOf,
    restoreRemovedFile,
    updatedFromDisk,
    isUpdatedFromDisk,
    clearUpdatedFromDisk,
    applyExternalContent,
    docHash,
    lastKnownDiskHash,
    stopSaveListener,
  };
}
