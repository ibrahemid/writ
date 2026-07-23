import { createSignal, createRoot } from "solid-js";
import type { EditorView } from "@codemirror/view";
import { checkSpelling, spellingAddIgnoredWord } from "../../services/tauri";
import { configStore } from "./config";
import type { SpellingLint } from "../../types/spelling";
import {
  setSpellingLints,
  clearSpellingLints,
  removeSpellingWord,
  spellingEntries,
  computeFixChanges,
  applySpellingFixes,
  type SpellingEntry,
} from "../../editor/spelling";

// Singleton state — Writ is single-window: one active editor view checks at a
// time. The store owns the debounced re-lint (with a generation counter that
// drops stale results) and exposes the live decoration set to the chip, the
// anchored menu, and the preview overlay.

const DEBOUNCE_MS = 400;

function createSpellingStore() {
  // Count of live decorations, published by the editor extension so it never
  // lags the visible underlines.
  const [count, setCount] = createSignal(0);
  // Whether the active buffer can be checked (Normal mode, under the size cap).
  // Drives the status-bar item's visibility independent of the on/off switch.
  const [eligible, setEligible] = createSignal(false);
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

  /**
   * Stops checking and clears the count/view without changing eligibility.
   * Used when the buffer is eligible but the feature is switched off.
   */
  function deactivate() {
    cancelTimer();
    generation += 1;
    view = null;
    setCount(0);
  }

  /** Full reset for buffer switch or editor teardown: also marks ineligible. */
  function detach() {
    deactivate();
    setEligible(false);
  }

  /** Called by the editor extension whenever the decoration set changes. */
  function publishCount(next: number) {
    setCount(next);
  }

  /** Schedules a re-lint, dropping any result an intervening edit supersedes. */
  function requestCheck(text: string) {
    cancelTimer();
    const requested = ++generation;
    if (text.length === 0) {
      applyResults(requested, []);
      return;
    }
    timer = setTimeout(() => {
      timer = null;
      void checkSpelling(text)
        .then((lints) => applyResults(requested, lints))
        .catch(() => {
          // Leave the prior decorations in place on failure.
        });
    }, DEBOUNCE_MS);
  }

  function applyResults(requested: number, lints: SpellingLint[]) {
    if (requested !== generation) return;
    const active = view;
    if (!active) return;
    active.dispatch({ effects: setSpellingLints.of(lints) });
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
    eligible,
    setEligible,
    attach,
    detach,
    deactivate,
    publishCount,
    requestCheck,
    clear,
    entries,
    fixAll,
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
