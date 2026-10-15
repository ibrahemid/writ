import { createSignal } from "solid-js";
import { EditorSelection, type Text, type TransactionSpec } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import type { FileOpenMode } from "../../types/buffer";
import {
  debouncedSave,
  forgetLandedText,
  cancelAutosave as cancelAutosaveService,
  currentSaveGeneration,
  flushAutosave as flushAutosaveService,
  holdUnsavedContent,
  materializePendingSave,
  peekHeldContent,
  onAutosaveSuccess,
  peekUnsavedContent,
  peekUnsavedText,
  releaseUnsavedContent,
  saveNow as saveNowService,
  materializeContent,
  type ChangeEncoder,
  type ContentSource,
  type DocumentSnapshot,
  type HeldContent,
  type SaveResult,
  type UnsavedText,
} from "../../services/autosave";
import {
  noteDiskState,
  restoreNoteFile,
  saveNoteTo,
  setBufferViewPosition,
  type NoteDiskAnswer,
  type UnmadeFile,
} from "../../services/tauri";
import { applyExternalDocument, reloadTransactionSpec, smallestChange } from "../../editor/external-reload";
import { TabStateCache, type TabState } from "../../editor/tab-state-cache";
import { PANE_IDS, type PaneId } from "../../lib/pane-layout";
import { programName, type VersionWriter } from "../../lib/writer";
import { holdRange, releaseRange } from "../../editor/held-ranges";
import { encodeChangesSince } from "../../editor/change-diff";
import { savesAsChanges, type FileOverrides } from "../../editor/large-file";
import { logFailure } from "../../lib/log";
import { saveStatusStore } from "../global/save-status";
import { sameDocument } from "../../editor/same-document";
import { readFailureOf, type ReadFailure } from "../../lib/read-failure";
import {
  detectFromContent as detectFromContentService,
  MAX_DETECT_BYTES,
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
      /** `document-changed`: an edit touched the range, or its view went away,
       * while the transform ran; the result was computed from text that is no
       * longer there, so nothing is replaced. */
      reason: "no-active-view" | "read-only" | "transform-error" | "document-changed";
      error?: unknown;
    };

export type EditorStore = ReturnType<typeof createEditorStore>;

const NOTHING_TO_SAVE: SaveResult = { ok: true, failures: [] };

/**
 * How long a note has to stop changing before its document is compared with
 * the text its file was last known to hold.
 *
 * The comparison ([`sameDocument`]) skips every subtree the two documents
 * share where their trees line up, which an edit and its undo leave them
 * doing, so it reads the stretches the edits since the save touched rather
 * than the document between them; it still waits for a pause rather than
 * running on every transaction. Nothing waits on it: a document that has
 * changed since its last comparison already reads dirty ([`isDirty`]).
 */
export const DIRTY_COMPARE_IDLE_MS = 150;

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
 * `docGeneration` counts transactions; `comparedGeneration` records the one
 * `matchesFile` was decided at. They diverge for as long as an edit has not
 * been compared, which is what makes an uncompared edit read dirty rather
 * than clean.
 */
interface NoteHashes {
  docGeneration: number;
  comparedGeneration: number;
  /**
   * Whether the document at `comparedGeneration` says what the file holds:
   * the open read it from the file, a write that landed carried it, or it
   * compared equal (`Text.eq`) with the text one of those left.
   */
  matchesFile: boolean;
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
  diskHash?: string;
}

const FRESH: NoteHashes = { docGeneration: 0, comparedGeneration: 0, matchesFile: false };

/**
 * A note a pane's view holds, with the documents its dirty answer is read
 * from. Only notes on screen keep a `Text`: a note with no view cannot change,
 * and keeping its document would hold a closed view's text in memory.
 */
interface ViewedNote {
  id: string;
  /** The newest document. */
  doc: Text;
  /** The newest document known to be on the file, when one is known. */
  saved: Text | null;
  /** The document the last open read, until that open answers. */
  opened: Text;
}

/** What a write that landed carried, as far as the dirty answer needs it. */
export interface SavedText {
  source: string | DocumentSnapshot;
  /** The note's save generation the text was queued at, for a write the autosave made. */
  generation?: number;
  /** Set by a caller that knows the text is the note's document as it stands when the write lands. */
  current?: boolean;
}

// Whether two documents are the same text, when that is plain without reading
// either of them.
function sameTextAtAGlance(a: Text, b: Text): boolean | null {
  if (a === b) return true;
  if (a.length !== b.length || a.lines !== b.lines) return false;
  return null;
}

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

/** Where a note was left: the caret, and the first position on the top line of the view. */
export interface ViewPosition {
  cursor: number;
  top: number;
}

/** A tier the note in front could move to, and the request to move it there. */
export interface TierChange {
  bufferId: string;
  mode: FileOpenMode;
}

const NO_OVERRIDES: FileOverrides = {};

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

const NORMAL_MODE: FileOpenMode = { kind: "Normal" };

// Where a kept editor state is filed: one per tab per pane (ADR-056).
function stateKey(pane: PaneId, id: string): string {
  return `${pane}\u0000${id}`;
}

function noteOfStateKey(key: string): string {
  return key.slice(key.indexOf("\u0000") + 1);
}

/** A pane's read of a note from disk, which the other pane waits on (ADR-056). */
interface NoteRead {
  readonly id: string;
  readonly done: Promise<void>;
  readonly end: () => void;
}

/**
 * What one pane's editor shows: its view, the note in it and what is read off
 * that note in front. Everything else the store holds is per note.
 */
function createPaneSlot(pane: PaneId) {
  const [cursorLine, setCursorLine] = createSignal(1);
  const [cursorCol, setCursorCol] = createSignal(1);
  const [lineCount, setLineCount] = createSignal(0);
  const [language, setLanguage] = createSignal<string | null>(null);
  const [selectionCount, setSelectionCount] = createSignal(1);
  const [largeFileMode, setLargeFileMode] = createSignal<FileOpenMode | null>(
    null,
  );
  // The longest line of the note in front as it was loaded, which decides
  // whether a LongLines file wraps and highlights ([`featuresFor`]).
  const [longestLine, setLongestLine] = createSignal(0);
  // A tier the document in front calls for after a large edit, which the chip
  // offers.
  const [tierOffer, setTierOffer] = createSignal<TierChange | null>(null);
  // Bumped on every change to the document in the view and whenever the view
  // takes another document. Nothing reads the text through it: a reader that
  // needs the text fetches it from the view ([`liveDoc`]) when it does its
  // work, after the keystroke rather than inside it.
  const [docVersion, setDocVersion] = createSignal(0);
  // Id of the buffer whose content is currently loaded into the pane's view.
  // Published by EditorInstance.loadBuffer only after the view holding the
  // buffer's content is registered, so [`liveDoc`] is that buffer's document
  // whenever this names it. The preview pane gates
  // rendering on this matching its own buffer id: during a tab switch
  // props.buffer.id flips reactively while the editor is still mid-load on the
  // outgoing buffer, and rendering then would cache the wrong buffer's HTML
  // under the incoming id (the #97 stale-cache flash).
  const [currentBufferId, setCurrentBufferId] = createSignal<string | null>(
    null,
  );
  // The note in front whose file could not be read; the editor shows the
  // reason in place of a document. Every load reads the file again.
  const [readFailure, setReadFailure] = createSignal<ReadFailure | null>(null);
  // Whether the pane's editor is mounted, which decides who registers the
  // editor's commands ([`commandPane`]).
  const [mounted, setMounted] = createSignal(false);
  // Whether the note in front takes the Markdown formatting commands.
  const [editsMarkdown, setEditsMarkdown] = createSignal(false);
  return {
    pane,
    cursorLine, setCursorLine,
    cursorCol, setCursorCol,
    lineCount, setLineCount,
    language, setLanguage,
    selectionCount, setSelectionCount,
    largeFileMode, setLargeFileMode,
    longestLine, setLongestLine,
    tierOffer, setTierOffer,
    docVersion, setDocVersion,
    currentBufferId, setCurrentBufferId,
    readFailure, setReadFailure,
    mounted, setMounted,
    editsMarkdown, setEditsMarkdown,
    view: null as EditorView | null,
    reading: null as NoteRead | null,
  };
}

type PaneSlot = ReturnType<typeof createPaneSlot>;

export interface EditorStoreOptions {
  /** The pane every top-level accessor answers for; the left one when not given. */
  focusedPane?: () => PaneId;
}

export function createEditorStore(options: EditorStoreOptions = {}) {
  const focusedPane = options.focusedPane ?? ((): PaneId => "left");
  const slots: Record<PaneId, PaneSlot> = { left: createPaneSlot("left"), right: createPaneSlot("right") };
  const focusedSlot = (): PaneSlot => slots[focusedPane()];
  // What the person turned on or off per file from the tier chip, for this
  // session. Kept by buffer id, so switching away and back keeps it.
  const [overrides, setOverrides] = createSignal<ReadonlyMap<string, FileOverrides>>(new Map());
  // The tier of each note loaded this session, which the tab tooltip names
  // when the status bar is hidden.
  const [tiers, setTiers] = createSignal<ReadonlyMap<string, FileOpenMode>>(new Map());
  // A tier switch the chip's offer was accepted for, for every editor holding
  // that note to carry out.
  const [tierSwitch, setTierSwitch] = createSignal<(TierChange & { seq: number }) | null>(null);
  let tierSeq = 0;
  // Where each note was last left in this window, so a tab whose cached
  // editor state is gone still reopens where it was.
  const viewPositions = new Map<string, ViewPosition>();
  const closeListeners = new Set<(id: string) => void>();
  // A monotonically-keyed request to reload the active buffer's content from
  // disk, raised when the file changed externally (audit blocker #53.4).
  // EditorInstance consumes it; the seq makes repeated external edits to the
  // same buffer each fire a fresh reload.
  // `writer` is what wrote the file, when the backend knows.
  const [externalReload, setExternalReload] = createSignal<{
    id: string;
    seq: number;
    writer?: VersionWriter | null;
  } | null>(null);
  let reloadSeq = 0;

  function requestExternalReload(bufferId: string, writer?: VersionWriter | null) {
    reloadSeq += 1;
    setExternalReload({ id: bufferId, seq: reloadSeq, writer: writer ?? null });
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
  const compareTimers = new Map<string, ReturnType<typeof setTimeout>>();
  // Handed out by `noteOpened`, one per call, and never reset. A counter that
  // started again would let a call read its own number off a record a later
  // call happens to have stamped with the same one.
  let openTickets = 0;
  // The notes the panes' views hold, by note: a note open in both panes is
  // one entry, its document shared.
  const viewed = new Map<string, ViewedNote>();
  // The save generations of the writes that carry each note's document as it
  // stands, emptied by the next edit, held or not. A write that lands with one
  // of them wrote what the note holds.
  const currentTextWrites = new Map<string, Set<number>>();
  // The notes whose hold keeps the note's document as it stands. A hold taken
  // from a queued or failed write the document had moved past is not one of
  // them, and every edit or load takes a note out until the store holds the
  // document again.
  const currentHolds = new Set<string>();

  function noteWriteOfCurrentText(id: string) {
    let writes = currentTextWrites.get(id);
    if (writes === undefined) {
      writes = new Set();
      currentTextWrites.set(id, writes);
    }
    writes.add(currentSaveGeneration(id));
  }

  function carriesCurrentText(id: string, generation: number | undefined): boolean {
    return generation !== undefined && currentTextWrites.get(id)?.has(generation) === true;
  }

  /**
   * Whether `unsaved` is the note's document as it stands. Text a hold keeps
   * is when the store held it as the document ([`holdText`]): a keystroke into
   * a held note and a switch away from one do, and a move into a hold that
   * carries an older queued or failed text does not. A queued or failed
   * write's text is only while that write carried the document and nothing
   * was typed since, because a failed text stays outstanding after a newer
   * write has gone out with newer text.
   */
  function isCurrentText(id: string, unsaved: UnsavedText): boolean {
    return unsaved.generation === null
      ? currentHolds.has(id)
      : carriesCurrentText(id, unsaved.generation);
  }

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
  const [removedText, setRemovedText] = createSignal<ReadonlyMap<string, HeldContent>>(
    new Map(),
  );

  // A note the launch handed back because its first file could not be made,
  // with the file it was to be. It has no file and never had one, so its bar
  // names that file rather than one that was deleted.
  const [unmadeFiles, setUnmadeFiles] = createSignal<ReadonlyMap<string, UnmadeFile>>(
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
    const kept = wantsText ? outstandingTextOf(id) : undefined;
    const writesKeptText = event === "moved" && kept !== undefined && isDirty(id);

    // The note may write again, and every way a hold ends has already dealt
    // with the text: two answers write it, the third replaces it on purpose, a
    // quiet reload only happens to a note with nothing of its own, and a file
    // found at another path is handled below. Held any longer it would come
    // back as a recovered note holding the version the person chose against,
    // so what the hold handed to the journal goes too, unless the write below
    // carries it.
    if (after === "present") {
      releaseUnsavedContent(id, writesKeptText);
      currentHolds.delete(id);
      // And the copy the next load of the tab would read in place of the file.
      // There is a file again, and it is the one the tab is about to hold.
      setRemovedText((current) => {
        if (!current.has(id)) return current;
        const next = new Map(current);
        next.delete(id);
        return next;
      });
      forgetUnmadeFile(id);
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
      if (kept !== undefined) holdText(id, kept.content, kept.current);
      return;
    }

    // A file found at another path is the one end that neither writes the
    // tab's text nor replaces it. The note is writable again at its new path,
    // and what it has been holding since the deletion is the only copy of it:
    // the queue went when the hold went on, so nothing else would write it.
    if (writesKeptText) {
      debouncedSave(id, kept.content, 0);
      if (kept.current) noteWriteOfCurrentText(id);
    }
  }

  function forgetUnmadeFile(id: string) {
    setUnmadeFiles((current) => {
      if (!current.has(id)) return current;
      const next = new Map(current);
      next.delete(id);
      return next;
    });
  }

  /** The view showing note `id`, the focused pane's first. */
  function viewOfNote(id: string): { slot: PaneSlot; view: EditorView } | null {
    const order = [focusedSlot(), ...PANE_IDS.map((pane) => slots[pane])];
    for (const slot of order) {
      if (slot.view !== null && slot.currentBufferId() === id) return { slot, view: slot.view };
    }
    return null;
  }

  /** The live document of `id`, when an editor is showing it. */
  function liveTextOfNote(id: string): string | undefined {
    return viewOfNote(id)?.view.state.doc.toString();
  }

  /**
   * The newest text of `id` that no file is known to hold: the live document,
   * else what autosave has outstanding, and whether it is the note's document
   * as it stands ([`isCurrentText`]).
   */
  function outstandingTextOf(id: string): { content: string; current: boolean } | undefined {
    const live = liveTextOfNote(id);
    if (live !== undefined) return { content: live, current: true };
    const unsaved = peekUnsavedText(id);
    if (unsaved === undefined) return undefined;
    return { content: unsaved.content, current: isCurrentText(id, unsaved) };
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
  function holdText(id: string, text: HeldContent, current: boolean, keepAfterMs?: number) {
    holdUnsavedContent(id, text, keepAfterMs);
    if (current) currentHolds.add(id);
    else currentHolds.delete(id);
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
   * left removed from the unsaved-text journal rather than from anything live.
   * So is `unmade`, for a note that never had a file and whose first one the
   * launch could not make: it is held the same way, and named by the file it
   * was to be. Everything else is [`recordFileEvent`]'s, including the
   * read-then-cancel order and the early return for a note already marked.
   */
  function markRemovedOnDisk(id: string, text?: string, unmade?: UnmadeFile) {
    if (isRemovedOnDisk(id)) return;
    recordFileEvent(id, "removed");
    if (unmade !== undefined) {
      setUnmadeFiles((current) => new Map(current).set(id, unmade));
    }
    if (text !== undefined) keepTextOfRemoved(id, text);
  }

  /**
   * The first file the launch could not make for `id`, while the note is
   * held without one; `null` for every other note.
   */
  function unmadeFileOf(id: string): UnmadeFile | null {
    return unmadeFiles().get(id) ?? null;
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
  // journal yet: `keepUnsavedForRecovery` reads it after this and cancels the
  // note, which is what takes it back out.
  function forgetFileState(id: string) {
    forgetUnmadeFile(id);
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
   * unsaved-text journal so closing the tab or quitting does not lose it and
   * handed to it on the autosave's debounce so a crash does not either
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
  // The program that wrote the file, when one did, by the name a person reads.
  const [updatedBy, setUpdatedBy] = createSignal<string | null>(null);
  let updatedFromDiskTimer: ReturnType<typeof setTimeout> | undefined;

  function markUpdatedFromDisk(id: string, program: string | null = null) {
    if (updatedFromDiskTimer !== undefined) clearTimeout(updatedFromDiskTimer);
    setUpdatedFromDisk(id);
    setUpdatedBy(program);
    updatedFromDiskTimer = setTimeout(() => {
      updatedFromDiskTimer = undefined;
      setUpdatedFromDisk(null);
      setUpdatedBy(null);
    }, UPDATED_FROM_DISK_MS);
  }

  /** The program that last wrote `id`'s file, while the marker for it shows. */
  function updatedFromDiskBy(id: string): string | null {
    return updatedFromDisk() === id ? updatedBy() : null;
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
   *
   * `writer` is what wrote the file. A connected program's text is marked in
   * the document, and the marker names the program.
   */
  function applyExternalContent(id: string, text: string, writer?: VersionWriter | null) {
    const shown = viewOfNote(id);
    const program = programName(writer);
    if (shown) {
      // The other pane's editor of the note takes the same change, mirrored.
      applyExternalDocument(shown.view, text, program);
      shown.slot.setLineCount(shown.view.state.doc.lines);
      noteOpened(id, shown.view.state.doc, true);
    } else {
      updateKeptStates(id, text, program);
      noteOpened(id, null, true);
    }
    // The note now holds what its file holds, so whatever was being asked
    // about the difference between them is answered.
    recordFileEvent(id, "settled");
    markUpdatedFromDisk(id, program);
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
    holdText(id, text, true);
  }

  /**
   * The newest text of `id` that no file is known to hold, and whether it is
   * the note's document as it stands.
   *
   * The live document when the note is the one on screen, else what its hold
   * is keeping, which is the document only when the store held it as that
   * ([`isCurrentText`]). Never a failed write's text: the caller asking this
   * is about to decide what to write, and a refusal that a later answer
   * superseded is not it ([`peekHeldContent`]).
   */
  function liveTextOf(id: string): { content: string; current: boolean } | undefined {
    const live = liveTextOfNote(id);
    if (live !== undefined) return { content: live, current: true };
    const held = peekHeldContent(id);
    return held === undefined ? undefined : { content: held, current: currentHolds.has(id) };
  }

  /** The kept text of a note that may not write, when there is one. */
  function textOfRemoved(id: string): string | undefined {
    const kept = removedText().get(id);
    return kept === undefined ? undefined : materializeContent(kept);
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
    saveStatusStore.noteRequestedSave(id);
    const written = saveNowService(id, text, restoreNoteFile);
    noteWriteOfCurrentText(id);
    const result = await written;
    if (result.ok) clearRemovedOnDisk(id);
    return result;
  }

  /**
   * Writes a held note with no file to `path`, a place the person picked,
   * which becomes its file ("Save As…").
   *
   * The same terms as [`restoreRemovedFile`]: the write goes through the
   * autosave service with a writer of its own, a failure reports to the same
   * bar every other save does, and the hold ends only once the write lands.
   */
  async function saveRemovedNoteTo(id: string, path: string): Promise<SaveResult> {
    const text = liveTextOfRemoved(id);
    if (text === undefined) return NOTHING_TO_SAVE;
    saveStatusStore.noteRequestedSave(id);
    const written = saveNowService(id, text, (bufferId, content) =>
      saveNoteTo(bufferId, content, path),
    );
    noteWriteOfCurrentText(id);
    const result = await written;
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

  function clearCompareTimer(id: string) {
    const timer = compareTimers.get(id);
    if (timer) {
      clearTimeout(timer);
      compareTimers.delete(id);
    }
  }

  function scheduleCompare(id: string) {
    clearCompareTimer(id);
    compareTimers.set(
      id,
      setTimeout(() => {
        compareTimers.delete(id);
        compareNow(id);
      }, DIRTY_COMPARE_IDLE_MS),
    );
  }

  // Decides `matchesFile` for the newest generation of the note in the view.
  // A note with no view has no document to compare, and keeps its answer.
  function compareNow(id: string) {
    const record = noteHashes().get(id);
    if (record === undefined || record.openInFlight === true) return;
    if (record.comparedGeneration === record.docGeneration) return;
    const note = viewed.get(id);
    if (note === undefined || note.saved === null) return;
    patchHashes(id, {
      comparedGeneration: record.docGeneration,
      matchesFile: sameDocument(note.doc, note.saved),
    });
  }

  /**
   * Records what a note holds the moment it is loaded or reloaded from disk.
   *
   * `opened` is the document the view holds right after the load, or null
   * for a note that is not in the view. `fromFile` says whether that text is
   * what Rust has just read from the note's file: text a hold kept, or typing
   * a save could not write, is not the file's and reads dirty.
   *
   * The file's digest comes from Rust, which read the file
   * (`writ_core::hash::comparison_digest_hex`), and is kept as the record of
   * what the file holds ([`lastKnownDiskHash`]). The frontend computes no
   * digest: whether the document says what the file says is the document
   * compared (`Text.eq`) with the text the file was last known to hold.
   *
   * A note with no file yet reads clean until it is typed into, which is
   * right for a new empty note; the first keystroke moves the generation and
   * the note reads dirty from there until a save gives it a file.
   *
   * A note whose file could not be described — not there, or its bytes not on
   * this machine — drops its record instead, the same as the call failing
   * outright. Either way the editor holds a document with nothing to compare
   * it against, and `isDirty` answers dirty for a note it holds no record of,
   * which is what stops a later reload replacing text no file holds.
   */
  function noteOpened(id: string, opened: Text | null, fromFile: boolean) {
    clearCompareTimer(id);
    currentTextWrites.delete(id);
    currentHolds.delete(id);
    const ticket = ++openTickets;
    setNoteHashes((current) => {
      const next = new Map(current);
      next.set(id, { ...FRESH, openTicket: ticket, openInFlight: true });
      return next;
    });
    if (opened !== null) viewed.set(id, { id, doc: opened, saved: null, opened });
    else viewed.delete(id);
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
        // it clean would let the file's return be reloaded over that text, so
        // it answers the way an undescribed file does and the person is asked.
        // The backend reaches this only for a note that names no file at all,
        // and this does not depend on that.
        if (savesAreHeld(id)) {
          forgetHashes(id);
          return;
        }
        settleOpen(id, opened, true, undefined);
        return;
      }
      settleOpen(id, opened, fromFile, answer.disk.hash);
    })();
  }

  // Fills the record an open left in flight.
  function settleOpen(
    id: string,
    opened: Text | null,
    fileHoldsOpened: boolean,
    diskHash: string | undefined,
  ) {
    const record = hashesOf(id);
    // A write that landed since this read went out knows the file better
    // than this read does, and it already recorded what it wrote. A
    // comparison it left for later decides nothing while the open is out, so
    // it is scheduled again now.
    if (record.writtenDuringOpen === true) {
      patchHashes(id, { openInFlight: false, writtenDuringOpen: false });
      if (record.comparedGeneration !== record.docGeneration) scheduleCompare(id);
      return;
    }
    const note = viewed.get(id);
    if (fileHoldsOpened && opened !== null && note !== undefined && note.opened === opened) {
      note.saved = opened;
    }
    const unedited = record.docGeneration === 0;
    patchHashes(id, {
      ...(diskHash === undefined ? {} : { diskHash }),
      comparedGeneration: 0,
      matchesFile: unedited && fileHoldsOpened,
      openInFlight: false,
      writtenDuringOpen: false,
    });
    // Typing that landed while the answer was out is compared with what the
    // open read, so undoing it reads clean again.
    if (!unedited) scheduleCompare(id);
  }

  /** Whether a later [`noteOpened`] has taken the note off this call's answer. */
  function supersededOpen(id: string, ticket: number): boolean {
    return hashesOf(id).openTicket !== ticket;
  }

  /**
   * Records that the document in the view changed, and compares it with the
   * text its file holds once the edits settle. Until then the note reads
   * dirty.
   */
  function noteEdited(id: string, doc: Text) {
    currentTextWrites.get(id)?.clear();
    currentHolds.delete(id);
    patchHashes(id, { docGeneration: hashesOf(id).docGeneration + 1 });
    const note = viewed.get(id);
    if (note !== undefined) note.doc = doc;
    else viewed.set(id, { id, doc, saved: null, opened: doc });
    scheduleCompare(id);
  }

  /**
   * Records what the note's file holds after a write landed on it.
   *
   * `diskHash` is the digest the save command computed over what it wrote, so
   * the record of the file comes from Rust here too, and a digest is Rust's
   * acknowledgement that the write is on the file. Null when the note had
   * nothing in it and no file to mint one for.
   *
   * `write` is what the write carried. For the note in the view, the document
   * the write was taken from becomes the text the file is known to hold, and
   * the note is clean while it still holds that document. A write handed over
   * as a string says what it wrote through its save generation: a write the
   * store issued with the document as it stands, with no edit since, wrote
   * that document. A note with no view is clean after such a write, or after one
   * whose caller knew it carried the note's current text (`current`), and
   * dirty after any other.
   *
   * `viaWriteBack` separates the two writes that can land here. The write-back
   * put the file at the note's path again, so the removed mark comes off here
   * rather than only where the restore was asked for: a restore requeued after
   * a failure lands from the autosave queue, and the bar would otherwise stay
   * over a file that is back. An ordinary save says only that there was a file
   * when it left ([`nextNoteFileState`]).
   */
  function noteSaved(
    id: string,
    diskHash: string | null,
    viaWriteBack: boolean,
    write?: SavedText,
  ) {
    recordFileEvent(id, viaWriteBack ? "settled" : "written");
    if (diskHash === null) return;
    const record = hashesOf(id);
    const newest = write?.current === true || carriesCurrentText(id, write?.generation);
    let matches: boolean | null;
    const note = viewed.get(id);
    if (note !== undefined) {
      const written =
        write === undefined
          ? null
          : typeof write.source === "object"
            ? (write.source as Text)
            : newest
              ? note.doc
              : null;
      note.saved = written;
      matches = written === null ? false : sameTextAtAGlance(note.doc, written);
    } else {
      matches = newest;
    }
    // An open that is still out read the file before this write changed it,
    // and its answer would otherwise land on top of this one.
    const duringOpen = record.openInFlight === true;
    patchHashes(id, {
      diskHash,
      ...(duringOpen ? { writtenDuringOpen: true } : {}),
      ...(matches === null
        ? { comparedGeneration: record.docGeneration - 1 }
        : { comparedGeneration: record.docGeneration, matchesFile: matches }),
    });
    if (matches === null) scheduleCompare(id);
  }

  /** Drops everything held for a note whose tab has gone. */
  function noteClosed(id: string) {
    clearCompareTimer(id);
    currentTextWrites.delete(id);
    currentHolds.delete(id);
    forgetHashes(id);
    forgetFileState(id);
    clearUpdatedFromDisk(id);
    forgetLandedText(id);
    for (const slot of PANE_IDS.map((pane) => slots[pane])) {
      if (slot.tierOffer()?.bufferId === id) slot.setTierOffer(null);
      tabStates.delete(stateKey(slot.pane, id));
    }
    viewed.delete(id);
    for (const listener of closeListeners) listener(id);
  }

  /** Hears every note whose tab closes; answers the call that stops it. */
  function onNoteClosed(listener: (id: string) => void): () => void {
    closeListeners.add(listener);
    return () => {
      closeListeners.delete(listener);
    };
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
   * same way: a note the tab is still opening would otherwise let a change
   * arriving in that window reload the file over text the tab is holding and
   * no file has.
   *
   * Reads stored answers only. The comparison that decides them runs after a
   * pause in typing ([`DIRTY_COMPARE_IDLE_MS`]), never here, since this sits
   * under every tab's mark.
   */
  function isDirty(id: string): boolean {
    const state = noteHashes().get(id);
    if (state === undefined) return true;
    if (state.openInFlight === true) return true;
    if (state.docGeneration !== state.comparedGeneration) return true;
    return !state.matchesFile;
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

  function lastKnownDiskHash(id: string): string | undefined {
    return noteHashes().get(id)?.diskHash;
  }

  // A landed write is the one thing that moves the record of the file without
  // the editor doing anything, so the store listens for it rather than making
  // every save path remember to report back.
  const stopSaveListener = onAutosaveSuccess(noteSaved);

  // A new view holds no note until its open says which. The note the pane
  // held before keeps its document only while the other pane shows it.
  function registerView(slot: PaneSlot, view: EditorView | null) {
    slot.view = view;
    const shownElsewhere = new Set(
      PANE_IDS.filter((pane) => pane !== slot.pane).map((pane) => slots[pane].currentBufferId()),
    );
    for (const id of [...viewed.keys()]) {
      if (!shownElsewhere.has(id)) viewed.delete(id);
    }
    slot.setDocVersion((version) => version + 1);
  }

  /**
   * The document another pane's view holds of note `id`, for a pane opening
   * the same note: it starts from that document rather than the file
   * (ADR-056).
   */
  function liveDocElsewhere(slot: PaneSlot, id: string): Text | null {
    for (const pane of PANE_IDS) {
      if (pane === slot.pane) continue;
      const other = slots[pane];
      if (other.view !== null && other.currentBufferId() === id) return other.view.state.doc;
    }
    return null;
  }

  /**
   * Marks `slot`'s editor as reading note `id` from disk, and answers the call
   * that ends the read. The other pane opening the note meanwhile waits for it
   * and starts from this pane's document, so a note shown in both panes is
   * read once (ADR-056).
   */
  function beginRead(slot: PaneSlot, id: string): () => void {
    slot.reading?.end();
    let resolve!: () => void;
    const done = new Promise<void>((settle) => {
      resolve = settle;
    });
    const read: NoteRead = {
      id,
      done,
      end: () => {
        if (slot.reading === read) slot.reading = null;
        resolve();
      },
    };
    slot.reading = read;
    return read.end;
  }

  /** The other pane's read of note `id` in flight, which ends once its view shows the note or the read fails. */
  function readElsewhere(slot: PaneSlot, id: string): Promise<void> | null {
    for (const pane of PANE_IDS) {
      if (pane === slot.pane) continue;
      const read = slots[pane].reading;
      if (read !== null && read.id === id) return read.done;
    }
    return null;
  }

  /**
   * A pane opening a note the other pane already shows takes that note's
   * record as it stands, and keeps a document to compare from when the
   * other pane let go of its own.
   */
  function noteShownTwice(id: string, doc: Text) {
    if (!viewed.has(id)) viewed.set(id, { id, doc, saved: null, opened: doc });
  }

  /**
   * Carries a change made in `slot`'s view of note `id` to the other pane:
   * into its view when it shows the note, else into the state it keeps for
   * the note. A document that does not line up with the change is not
   * touched: its kept state is dropped and opens from the file.
   */
  function mirrorChanges(slot: PaneSlot, id: string, startLength: number, spec: TransactionSpec) {
    for (const pane of PANE_IDS) {
      if (pane === slot.pane) continue;
      const other = slots[pane];
      if (other.view !== null && other.currentBufferId() === id) {
        if (other.view.state.doc.length === startLength) other.view.dispatch(spec);
        else logFailure("a file open in both panes no longer matched in one of them");
        continue;
      }
      const key = stateKey(pane, id);
      const kept = tabStates.peek(key);
      if (kept === undefined) continue;
      if (kept.state.doc.length === startLength) tabStates.update(key, kept.state.update(spec).state);
      else tabStates.delete(key);
    }
  }

  /** The document in the view, for a reader doing its work after a change. */
  function liveDoc(slot: PaneSlot): Text | null {
    return slot.view?.state.doc ?? null;
  }

  function getActiveText(
    slot: PaneSlot,
    useSelectionIfPresent: boolean,
  ): { text: string; usedSelection: boolean } | null {
    const view = slot.view;
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
    slot: PaneSlot,
    useSelectionIfPresent: boolean,
  ): { from: number; to: number; text: string; usedSelection: boolean } | null {
    const view = slot.view;
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
  function replaceRange(slot: PaneSlot, from: number, to: number, insert: string): boolean {
    const view = slot.view;
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
    slot: PaneSlot,
    options: ApplyEditOptions,
  ): Promise<ApplyEditResult> {
    const view = slot.view;
    if (!view) return { applied: false, reason: "no-active-view" };

    if (view.state.readOnly) return { applied: false, reason: "read-only" };

    const main = view.state.selection.main;
    const useSelection = options.useSelectionIfPresent && !main.empty;
    const start = useSelection ? main.from : 0;
    const end = useSelection ? main.to : view.state.doc.length;
    const input = view.state.doc.sliceString(start, end);
    // The transform answers later; typing in the meantime moves the text the
    // range covers, and the result must land where that text is then.
    const held = holdRange(view, start, end);

    let output: string;
    try {
      output = await options.transform(input);
    } catch (error) {
      releaseRange(view, held);
      return { applied: false, reason: "transform-error", error };
    }

    const range = releaseRange(view, held);
    if (range === null || !view.dom.isConnected) {
      return { applied: false, reason: "document-changed" };
    }
    if (view.state.readOnly) return { applied: false, reason: "read-only" };
    view.dispatch({
      changes: { from: range.from, to: range.to, insert: output },
      selection: EditorSelection.single(range.from, range.from + output.length),
    });
    view.focus();

    return {
      applied: true,
      usedSelection: useSelection,
      outputLength: output.length,
    };
  }

  /** The person's choices for one file, over what its tier gives it. */
  function overridesOf(id: string): FileOverrides {
    return overrides().get(id) ?? NO_OVERRIDES;
  }

  function setOverride(id: string, feature: keyof FileOverrides, on: boolean) {
    setOverrides((current) => {
      const next = new Map(current);
      next.set(id, { ...(current.get(id) ?? NO_OVERRIDES), [feature]: on });
      return next;
    });
  }

  /** The tier a note opened in this session, or null for one not loaded yet. */
  function tierOf(id: string): FileOpenMode | null {
    return tiers().get(id) ?? null;
  }

  function recordTier(id: string, mode: FileOpenMode) {
    if (tiers().get(id)?.kind === mode.kind) return;
    setTiers((current) => new Map(current).set(id, mode));
  }

  /** Offers `change` from the chip, or withdraws the offer with null. */
  function offerTier(slot: PaneSlot, change: TierChange | null) {
    const current = slot.tierOffer();
    if (change === null ? current === null : current?.bufferId === change.bufferId && current.mode.kind === change.mode.kind) return;
    slot.setTierOffer(change);
  }

  /** Takes the offered tier: every editor holding the note moves it there. */
  function acceptTierOffer(slot: PaneSlot) {
    const offer = slot.tierOffer();
    if (offer === null) return;
    slot.setTierOffer(null);
    setTierSwitch({ ...offer, seq: ++tierSeq });
  }

  /**
   * Records where note `id` was left. `persist` also writes it to the note's
   * row, so a tab opened on it in a later session starts there.
   */
  function rememberViewPosition(id: string, position: ViewPosition, persist: boolean) {
    const last = viewPositions.get(id);
    viewPositions.set(id, position);
    if (!persist || (last?.cursor === position.cursor && last.top === position.top)) return;
    // Where a note was left is a convenience: nothing about it may stop a tab
    // switch or a close, so a failure of any kind is a log line.
    const fail = () => logFailure("the place a file was left could not be kept");
    try {
      void setBufferViewPosition(id, position.cursor, position.top).catch(fail);
    } catch {
      fail();
    }
  }

  function viewPositionOf(id: string): ViewPosition | undefined {
    return viewPositions.get(id);
  }

  /**
   * Lets go of the document a note's last write landed with, for a note whose
   * editor state the window no longer keeps: its next save sends the whole
   * text.
   */
  function releaseDocument(id: string) {
    forgetLandedText(id);
  }

  // The editor states of the open tabs behind the one in front in each pane
  // (EP-06), under one budget (ADR-022). A state dropped to keep within it lets
  // go of the document its note's last write landed with too, once no editor
  // holds that note any more.
  const tabStates = new TabStateCache(undefined, (key) => {
    const id = noteOfStateKey(key);
    if (viewOfNote(id) !== null) return;
    if (PANE_IDS.some((pane) => tabStates.has(stateKey(pane, id)))) return;
    releaseDocument(id);
  });

  /**
   * Takes a file's new text into every state a pane keeps for note `id`, as
   * one change that can be undone, so switching back keeps the tab's history.
   * A program's text is marked there too, and shows when the tab is switched
   * to.
   */
  function updateKeptStates(id: string, content: string, program: string | null = null) {
    for (const pane of PANE_IDS) {
      const key = stateKey(pane, id);
      const kept = tabStates.peek(key);
      if (kept === undefined) continue;
      const before = kept.state.doc.toString();
      const after = content.indexOf("\r") === -1 ? content : content.replace(/\r\n?/g, "\n");
      if (before === after) continue;
      tabStates.update(key, kept.state.update(reloadTransactionSpec(smallestChange(before, after), program)).state);
    }
  }

  /**
   * Hands what pane `from` holds of note `id` to pane `to`: the live editor's
   * state when `from` shows the note, else the state it keeps. The state was
   * built by the other pane's editor, which `foreign` tells the taker.
   */
  function moveTabState(from: PaneId, to: PaneId, id: string) {
    const source = slots[from];
    let entry: TabState | undefined;
    if (source.view !== null && source.currentBufferId() === id) {
      entry = {
        state: source.view.state,
        scroll: null,
        mode: source.largeFileMode() ?? NORMAL_MODE,
        longest: source.longestLine(),
      };
    } else {
      entry = tabStates.take(stateKey(from, id));
    }
    if (entry === undefined) return;
    const target = slots[to];
    if (target.view !== null && target.currentBufferId() === id) return;
    tabStates.put(stateKey(to, id), { ...entry, scroll: null, foreign: true });
  }

  /** Drops the state pane `pane` keeps for note `id`. */
  function dropTabState(pane: PaneId, id: string) {
    tabStates.delete(stateKey(pane, id));
  }

  // The large tiers save by sending their changes (ADR-022). Only a note in
  // front of a pane has a tier here, and only its view schedules saves
  // through this.
  function changeEncoderFor(bufferId: string): ChangeEncoder | undefined {
    const shown = viewOfNote(bufferId);
    return shown !== null && savesAsChanges(shown.slot.largeFileMode())
      ? encodeChangesSince
      : undefined;
  }

  // Autosave and language detection are services; the editor component routes
  // through these so it only ever talks to its store (layering rule).
  function scheduleAutosave(
    bufferId: string,
    content: ContentSource,
    delayMs: number,
    current = true,
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
    // reads and no write path does, which hands it to the unsaved-text journal
    // on this same debounce, and for a note whose file is gone to the copy the
    // next load of its tab reads ([`holdText`]).
    //
    // A snapshot is held as it is: it is the document as it stood, so no
    // keystroke turns the document into a string. A getter is read here rather
    // than held. It reads the editor's single view, and the next tab switch
    // destroys that view and builds another note's in its place, so a held
    // getter would report the incoming note's document under this one's name.
    //
    // The text is the caller's newest, so the writes issued before it are no
    // longer known to carry the same text. It is the note's document unless
    // the caller says otherwise (`current`): Keep mine hands over held text
    // the store could not vouch for.
    currentTextWrites.get(bufferId)?.clear();
    if (savesAreHeld(bufferId)) {
      holdText(bufferId, typeof content === "function" ? content() : content, current, delayMs);
      return;
    }
    debouncedSave(bufferId, content, delayMs, changeEncoderFor(bufferId));
    if (current) noteWriteOfCurrentText(bufferId);
  }

  function cancelAutosave(bufferId: string) {
    cancelAutosaveService(bufferId);
  }

  function flushAutosave(bufferId?: string): Promise<SaveResult> {
    return flushAutosaveService(bufferId);
  }

  /**
   * Leaves a note's queued typing queued, as text rather than as a reader of
   * the view, for a view that is about to go. Only an explicit discard
   * cancels ([`cancelAutosave`]).
   */
  function keepQueuedText(bufferId: string) {
    materializePendingSave(bufferId);
  }

  /** The newest text of a note that is not known to be on disk. */
  function unsavedTextOf(id: string): string | undefined {
    return peekUnsavedContent(id);
  }

  /** Records that the load of `bufferId` could not read its file. */
  function failRead(slot: PaneSlot, bufferId: string, error: unknown): ReadFailure {
    const failure = readFailureOf(bufferId, error);
    slot.setReadFailure(failure);
    return failure;
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
  function saveActiveBuffer(slot: PaneSlot): Promise<SaveResult> {
    const bufferId = slot.currentBufferId();
    const view = slot.view;
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
    if (slot.largeFileMode()?.kind === "Binary")
      return Promise.resolve(NOTHING_TO_SAVE);
    if (isRemovedOnDisk(bufferId)) return restoreRemovedFile(bufferId);
    saveStatusStore.noteRequestedSave(bufferId);
    const written = saveNowService(bufferId, view.state.doc, undefined, changeEncoderFor(bufferId));
    noteWriteOfCurrentText(bufferId);
    return written;
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
   *
   * The write counts as carrying the note's document only when the text it
   * repeats is that document ([`isCurrentText`]). A failed text pressed again
   * while a newer write is on its way is older than the note.
   */
  function retrySave(id: string): Promise<SaveResult> {
    if (isRemovedOnDisk(id)) return restoreRemovedFile(id);
    if (savesAreHeld(id)) return Promise.resolve(NOTHING_TO_SAVE);
    const unsaved = peekUnsavedText(id);
    if (unsaved === undefined) return Promise.resolve(NOTHING_TO_SAVE);
    const current = isCurrentText(id, unsaved);
    saveStatusStore.noteRequestedSave(id);
    const written = saveNowService(id, unsaved.content);
    if (current) noteWriteOfCurrentText(id);
    return written;
  }

  /** What the backend can say about the note's file right now. */
  function readDiskState(id: string): Promise<NoteDiskAnswer> {
    return noteDiskState(id);
  }

  // Detection reads the head of the text only, so only the head is
  // materialized.
  function detectFromDocument(doc: Text): string | null {
    return detectFromContentService(doc.sliceString(0, MAX_DETECT_BYTES));
  }

  /**
   * The store as seen from one pane: everything per note is the same, and
   * what is read off the editor (`getView`, `currentBufferId`, the cursor, the
   * tier in front, `readFailure`) is that pane's.
   */
  function scoped(slotOf: () => PaneSlot) {
    return {
      cursorLine: () => slotOf().cursorLine(),
      setCursorLine: (line: number) => slotOf().setCursorLine(line),
      cursorCol: () => slotOf().cursorCol(),
      setCursorCol: (col: number) => slotOf().setCursorCol(col),
      lineCount: () => slotOf().lineCount(),
      setLineCount: (count: number) => slotOf().setLineCount(count),
      language: () => slotOf().language(),
      setLanguage: (lang: string | null) => slotOf().setLanguage(lang),
      selectionCount: () => slotOf().selectionCount(),
      setSelectionCount: (count: number) => slotOf().setSelectionCount(count),
      docVersion: () => slotOf().docVersion(),
      noteDocChanged: () => slotOf().setDocVersion((version) => version + 1),
      liveDoc: () => liveDoc(slotOf()),
      currentBufferId: () => slotOf().currentBufferId(),
      setCurrentBufferId: (id: string | null) => slotOf().setCurrentBufferId(id),
      externalReload, requestExternalReload,
      pendingReveal, requestReveal, clearReveal,
      largeFileMode: () => slotOf().largeFileMode(),
      setLargeFileMode: (mode: FileOpenMode | null) => slotOf().setLargeFileMode(mode),
      longestLine: () => slotOf().longestLine(),
      setLongestLine: (longest: number) => slotOf().setLongestLine(longest),
      overridesOf, setOverride,
      tierOf, recordTier,
      tierOffer: () => slotOf().tierOffer(),
      offerTier: (change: TierChange | null) => offerTier(slotOf(), change),
      acceptTierOffer: () => acceptTierOffer(slotOf()),
      tierSwitch,
      rememberViewPosition, viewPositionOf, releaseDocument,
      registerView: (view: EditorView | null) => registerView(slotOf(), view),
      getView: (): EditorView | null => slotOf().view,
      focusEditor: () => slotOf().view?.focus(),
      getActiveText: (useSelectionIfPresent: boolean) => getActiveText(slotOf(), useSelectionIfPresent),
      getSelectionRange: (useSelectionIfPresent: boolean) => getSelectionRange(slotOf(), useSelectionIfPresent),
      replaceRange: (from: number, to: number, insert: string) => replaceRange(slotOf(), from, to, insert),
      applyEditToActiveBuffer: (editOptions: ApplyEditOptions) => applyEditToActiveBuffer(slotOf(), editOptions),
      scheduleAutosave,
      cancelAutosave,
      flushAutosave,
      keepQueuedText,
      unsavedTextOf,
      readFailure: () => slotOf().readFailure(),
      failRead: (bufferId: string, error: unknown) => failRead(slotOf(), bufferId, error),
      clearReadFailure: () => slotOf().setReadFailure(null),
      saveActiveBuffer: () => saveActiveBuffer(slotOf()),
      savesAreHeld,
      retrySave,
      readDiskState,
      detectFromDocument,
      noteOpened,
      noteEdited,
      noteSaved,
      noteClosed,
      onNoteClosed,
      isDirty,
      isTracked,
      noteFileStates,
      noteFileState,
      recordFileEvent,
      pendingChangeAnswer,
      isRemovedOnDisk,
      isFileChangedOnDisk,
      markRemovedOnDisk,
      unmadeFileOf,
      clearRemovedOnDisk,
      keepTextOfRemoved,
      textOfRemoved,
      liveTextOf,
      restoreRemovedFile,
      saveRemovedNoteTo,
      updatedFromDisk,
      isUpdatedFromDisk,
      updatedFromDiskBy,
      clearUpdatedFromDisk,
      applyExternalContent,
      lastKnownDiskHash,
      stopSaveListener,
      moveTabState,
      dropTabState,
      /** The pane whose editor registers the editor's commands: the left one while it is mounted. */
      commandPane: (): PaneId | null =>
        slots.left.mounted() ? "left" : slots.right.mounted() ? "right" : null,
      /** Whether the focused pane's note takes the Markdown formatting commands. */
      editsMarkdown: () => slotOf().editsMarkdown(),
    };
  }

  /** What only an editor of one pane does: its own view, states and mirror. */
  function paneOwn(slot: PaneSlot) {
    return {
      pane: slot.pane,
      setMounted: (mounted: boolean) => slot.setMounted(mounted),
      setEditsMarkdown: (edits: boolean) => slot.setEditsMarkdown(edits),
      liveDocElsewhere: (id: string) => liveDocElsewhere(slot, id),
      beginRead: (id: string) => beginRead(slot, id),
      readElsewhere: (id: string) => readElsewhere(slot, id),
      /** Ends this pane's read, for an editor going away mid-read. */
      endRead: () => slot.reading?.end(),
      noteShownTwice,
      mirrorChanges: (id: string, startLength: number, spec: TransactionSpec) =>
        mirrorChanges(slot, id, startLength, spec),
      keepTabState: (id: string, entry: TabState) => tabStates.put(stateKey(slot.pane, id), entry),
      takeTabState: (id: string) => tabStates.take(stateKey(slot.pane, id)),
      /** Lets go of every state this pane keeps, for an editor going away. */
      clearTabStates: () => {
        for (const id of tabStatesOf(slot.pane)) tabStates.delete(stateKey(slot.pane, id));
      },
      updateKeptStates,
    };
  }

  function tabStatesOf(pane: PaneId): string[] {
    const prefix = stateKey(pane, "");
    return tabStates.keys().filter((key) => key.startsWith(prefix)).map(noteOfStateKey);
  }

  return {
    ...scoped(focusedSlot),
    /** The store as pane `pane`'s editor reads and writes it. */
    forPane: (pane: PaneId) => ({ ...scoped(() => slots[pane]), ...paneOwn(slots[pane]) }),
    /** The kept states, for the tests that count them. */
    keptStateCount: () => tabStates.keys().length,
  };
}
