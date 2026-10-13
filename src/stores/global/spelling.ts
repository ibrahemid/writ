import { createSignal, createRoot } from "solid-js";
import type { EditorView } from "@codemirror/view";
import { checkSpelling, spellingAddIgnoredWord } from "../../services/tauri";
import { configStore } from "./config";
import type { SpellingLint } from "../../types/spelling";
import {
  setSpellingLintsIn,
  clearSpellingLints,
  removeSpellingWord,
  spellingEntries,
  checkRanges,
  computeFixChanges,
  computeChosenFix,
  applySpellingFixes,
  type CheckedRange,
  type SpellingEntry,
} from "../../editor/spelling";

// Singleton state — Writ is single-window: one active editor view checks at a
// time. The store owns the debounced re-lint (with a generation counter that
// drops stale results) and exposes the live decoration set to the chip, the
// anchored menu, and the preview overlay. A re-lint sends what was edited
// since the last one, each stretch widened to its paragraph; edits that leave
// more stretches than one check sends (a Replace All) go as one stretch from
// the first to the last.

const DEBOUNCE_MS = 400;

function createSpellingStore() {
  // Count of live decorations, published by the editor extension so it never
  // lags the visible underlines.
  const [count, setCount] = createSignal(0);
  let view: EditorView | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let generation = 0;

  function cancelTimer() {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
  }

  /** Binds the active editor view. Called when spell check turns on for a buffer. */
  function attach(next: EditorView) {
    view = next;
  }

  /** Stops checking and clears the count/view. Used when the feature is switched off. */
  function deactivate() {
    cancelTimer();
    generation += 1;
    view = null;
    setCount(0);
  }

  /** Full reset for buffer switch or editor teardown. */
  function detach() {
    deactivate();
  }

  /** Called by the editor extension whenever the decoration set changes. */
  function publishCount(next: number) {
    setCount(next);
  }

  /**
   * Schedules a re-lint of what changed since the last one, dropping any
   * result an intervening edit supersedes. The stretches are read from the
   * view when the debounce fires.
   */
  function requestCheck() {
    cancelTimer();
    const requested = ++generation;
    timer = setTimeout(() => {
      timer = null;
      void checkPending(requested);
    }, DEBOUNCE_MS);
  }

  async function checkPending(requested: number) {
    const active = view;
    if (!active) return;
    const state = active.state;
    const ranges = checkRanges(state);
    if (ranges.length === 0) return;
    let checked: CheckedRange[];
    try {
      checked = await Promise.all(
        ranges.map(async (range): Promise<CheckedRange> => {
          const text = state.doc.sliceString(range.from, range.to);
          const lints = text.length === 0 ? [] : await checkSpelling(text);
          return { ...range, lints: lints.map((lint) => offsetLint(lint, range.from)) };
        }),
      );
    } catch {
      // Leave the prior decorations in place on failure.
      return;
    }
    // A later edit has its own request coming, and the stretches this one
    // sent stay pending for it.
    if (requested !== generation || view !== active || active.state.doc !== state.doc) return;
    active.dispatch({ effects: setSpellingLintsIn.of(checked) });
  }

  function offsetLint(lint: SpellingLint, by: number): SpellingLint {
    return { ...lint, fromUtf16: lint.fromUtf16 + by, toUtf16: lint.toUtf16 + by };
  }

  /** Drops all decorations without a re-lint (used when turning the feature off). */
  function clear() {
    cancelTimer();
    generation += 1;
    view?.dispatch({ effects: clearSpellingLints.of() });
    setCount(0);
  }

  /** Current, remapped entries — the source of truth for menus and fixes. */
  function entries(): SpellingEntry[] {
    return view ? spellingEntries(view.state) : [];
  }

  function docSlice(from: number, to: number): string {
    return view ? view.state.doc.sliceString(from, to) : "";
  }

  /** Applies every confident, still-matching fix in one undo step. */
  function fixAll(): number {
    const active = view;
    if (!active) return 0;
    const fixes = computeFixChanges(entries(), docSlice);
    const applied = applySpellingFixes(active, fixes);
    active.focus();
    return applied;
  }

  /**
   * Replaces one flagged word with a suggestion the user picked.
   *
   * The missing single-word action: until now a user could fix everything at
   * once or ignore a word, but not accept one correction. Returns false when
   * the text moved on and the entry no longer matches.
   */
  function applyOne(entry: SpellingEntry, replacement: string): boolean {
    const active = view;
    if (!active) return false;
    const fix = computeChosenFix(entry, docSlice, replacement);
    if (!fix) return false;
    applySpellingFixes(active, [fix]);
    active.focus();
    return true;
  }

  /** Applies only the fixes for the given entries, in one undo step. */
  function applyEntries(selected: SpellingEntry[]): number {
    const active = view;
    if (!active) return 0;
    const wanted = new Set(selected.map(entryKey));
    const fixes = computeFixChanges(entries(), docSlice, (entry) => wanted.has(entryKey(entry)));
    return applySpellingFixes(active, fixes);
  }

  /** Moves the cursor to an entry and scrolls it into view. */
  function reveal(entry: SpellingEntry) {
    const active = view;
    if (!active) return;
    const docLen = active.state.doc.length;
    const anchor = Math.min(Math.max(entry.from, 0), docLen);
    active.dispatch({ selection: { anchor }, scrollIntoView: true });
    active.focus();
  }

  /** Adds a word to the ignore list and drops its decorations immediately. */
  async function ignoreWord(word: string) {
    view?.dispatch({ effects: removeSpellingWord.of(word) });
    await spellingAddIgnoredWord(word);
    // The command persisted the word server-side; resync so a later
    // update_config write does not clobber it with the stale in-memory list.
    await configStore.load();
  }

  return {
    count,
    attach,
    detach,
    deactivate,
    publishCount,
    requestCheck,
    clear,
    entries,
    fixAll,
    applyOne,
    applyEntries,
    reveal,
    ignoreWord,
  };
}

/** Stable identity for an entry across a selection set. */
export function entryKey(entry: SpellingEntry): string {
  return `${entry.from}:${entry.to}:${entry.word}`;
}

export type SpellingStore = ReturnType<typeof createSpellingStore>;
export const spellingStore = createRoot(createSpellingStore);
