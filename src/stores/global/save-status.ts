import { createSignal, createRoot } from "solid-js";
import {
  currentSaveGeneration,
  onAutosaveError,
  onAutosaveStart,
  onAutosaveSuccess,
} from "../../services/autosave";
import {
  describeSaveFailure,
  type SaveFailureReason,
} from "../../lib/save-error";
import { noteName } from "../../lib/note-name";
import { windowRegistry } from "./window-registry";

// Singleton — app-global, not window-scoped (ADR-009 E3). Autosave runs once
// for the whole app, so the record of what each of its writes did belongs
// beside it. The state itself is per note: with more than one tab open, one
// status for all of them names no file and is wrong for every tab but the one
// that last wrote.

/**
 * Where one note stands with its file.
 *
 * `dirty` and `clean` come from the document/file comparison the editor owns
 * (`editorStore.isDirty`); the other three come from the write itself.
 */
export type SaveState = "clean" | "dirty" | "saving" | "saved" | "failed";

export interface NoteSaveStatus {
  state: SaveState;
  reason?: SaveFailureReason;
  fileName: string;
}

const SAVED_VISIBLE_MS = 1200;

function createSaveStatusStore() {
  const [failures, setFailures] = createSignal<
    ReadonlyMap<string, SaveFailureReason>
  >(new Map());
  const [writing, setWriting] = createSignal<ReadonlySet<string>>(new Set());
  const [justSaved, setJustSaved] = createSignal<ReadonlySet<string>>(
    new Set(),
  );
  const savedTimers = new Map<string, ReturnType<typeof setTimeout>>();
  // Per note, the newest write generation whose failure says nothing about the
  // note any more ([`forgetWritesSoFar`]). Read only when a failure arrives.
  const answeredWrites = new Map<string, number>();

  function withoutId<T>(
    set: (
      fn: (current: ReadonlyMap<string, T>) => ReadonlyMap<string, T>,
    ) => void,
    id: string,
  ) {
    set((current) => {
      if (!current.has(id)) return current;
      const next = new Map(current);
      next.delete(id);
      return next;
    });
  }

  function addTo(
    set: (fn: (current: ReadonlySet<string>) => ReadonlySet<string>) => void,
    id: string,
  ) {
    set((current) => new Set(current).add(id));
  }

  function removeFrom(
    set: (fn: (current: ReadonlySet<string>) => ReadonlySet<string>) => void,
    id: string,
  ) {
    set((current) => {
      if (!current.has(id)) return current;
      const next = new Set(current);
      next.delete(id);
      return next;
    });
  }

  function clearSavedTimer(id: string) {
    const timer = savedTimers.get(id);
    if (timer) {
      clearTimeout(timer);
      savedTimers.delete(id);
    }
  }

  onAutosaveStart((id) => {
    addTo(setWriting, id);
  });

  onAutosaveSuccess((id) => {
    removeFrom(setWriting, id);
    withoutId(setFailures, id);
    clearSavedTimer(id);
    addTo(setJustSaved, id);
    savedTimers.set(
      id,
      setTimeout(() => {
        savedTimers.delete(id);
        removeFrom(setJustSaved, id);
      }, SAVED_VISIBLE_MS),
    );
  });

  onAutosaveError((id, error, generation) => {
    // The write is over whatever becomes of its reason, so the note stops
    // reading as writing either way.
    removeFrom(setWriting, id);
    clearSavedTimer(id);
    removeFrom(setJustSaved, id);
    if (writeWasAnsweredFor(id, generation)) return;
    setFailures((current) =>
      new Map(current).set(id, describeSaveFailure(error)),
    );
  });

  /**
   * Whether the write that failed is one something has already answered for.
   *
   * A generation at or below the mark was on the wire before the answer, so
   * its refusal is about a file the answer has since dealt with; one above it
   * was issued afterwards and is news.
   *
   * A mark above the note's current generation can only mean the counter was
   * reset underneath us, which happens when a test drives autosave in
   * isolation. The mark is meaningless then and goes, rather than silently
   * swallowing every failure that follows.
   */
  function writeWasAnsweredFor(id: string, generation: number): boolean {
    const mark = answeredWrites.get(id);
    if (mark === undefined) return false;
    if (currentSaveGeneration(id) < mark) {
      answeredWrites.delete(id);
      return false;
    }
    return generation <= mark;
  }

  // The comparison lives in the window's editor store and this store is
  // app-global, so it is reached through the registry rather than imported: a
  // global store may not depend on a window one (ADR-009 E3).
  //
  // No window registered, and a window whose editor is not built yet, both
  // answer the same way: nothing is known to differ from its file.
  //
  // `isDirty` answers `true` for a note it holds no record of, which is the
  // fail-closed answer a reload decision needs and the wrong one to draw a
  // mark from: a tab restored at launch and never brought to the front has no
  // record and no unsaved text either. So the mark asks whether the note is
  // tracked before it asks whether it differs.
  function documentDiffersFromFile(id: string): boolean {
    const editor = windowRegistry.getActive()?.editor as
      | {
          isDirty?: (id: string) => boolean;
          isTracked?: (id: string) => boolean;
        }
      | undefined;
    if (editor?.isTracked?.(id) !== true) return false;
    return editor.isDirty?.(id) ?? false;
  }

  /**
   * Where note `id` stands, without looking up what it is called.
   *
   * The per-tab mark needs the state and nothing else, and it renders once per
   * open tab.
   *
   * `saved` is the one state that claims the file holds what the person is
   * looking at, so it is the one state a document that differs may not show:
   * type, pause for the write, type again, and the seconds that follow would
   * otherwise read `Saved` over text no file has. `saving` outranks `dirty`
   * deliberately — during any write in flight the document differs by
   * construction, so a `dirty` that won there would leave `saving` unreachable
   * — and it claims nothing about the file, while the tab keeps its mark
   * throughout.
   */
  function stateOf(id: string): SaveState {
    if (failures().has(id)) return "failed";
    if (writing().has(id)) return "saving";
    const differs = documentDiffersFromFile(id);
    if (!differs && justSaved().has(id)) return "saved";
    return differs ? "dirty" : "clean";
  }

  /** Where note `id` stands, right now, and what it is called. */
  function forNote(id: string): NoteSaveStatus {
    const state = stateOf(id);
    const fileName = noteName(id);
    const reason = failures().get(id);
    return reason !== undefined
      ? { state, reason, fileName }
      : { state, fileName };
  }

  /** The failure note `id` is showing, if it is showing one. */
  function failureFor(id: string): SaveFailureReason | undefined {
    return failures().get(id);
  }

  /** Drops everything held for a note whose tab has gone. */
  function forgetNote(id: string) {
    clearSavedTimer(id);
    withoutId(setFailures, id);
    removeFrom(setWriting, id);
    removeFrom(setJustSaved, id);
    answeredWrites.delete(id);
  }

  /**
   * Drops the note's failure and every failure still to arrive about a write
   * already on the wire.
   *
   * For the moment something answers for the note's file: a bar about a write
   * the answer has just made irrelevant may not sit on the tab afterwards
   * saying the note could not be written when it has just been. Clearing what
   * is on screen is not enough on its own, because a write issued before the
   * answer can still be in flight and its refusal lands after this returns.
   * Everything issued afterwards is about the file as the answer left it and
   * still shows.
   */
  function forgetWritesSoFar(id: string) {
    answeredWrites.set(id, currentSaveGeneration(id));
    clearSavedTimer(id);
    withoutId(setFailures, id);
    removeFrom(setWriting, id);
    removeFrom(setJustSaved, id);
  }

  /** Drops every record. For a test driving the store in isolation. */
  function reset() {
    for (const timer of savedTimers.values()) clearTimeout(timer);
    savedTimers.clear();
    answeredWrites.clear();
    setFailures(new Map<string, SaveFailureReason>());
    setWriting(new Set<string>());
    setJustSaved(new Set<string>());
  }

  return {
    stateOf,
    forNote,
    failureFor,
    forgetNote,
    forgetWritesSoFar,
    reset,
  };
}

export const saveStatusStore = createRoot(createSaveStatusStore);
