import { createEffect, createSignal, on } from "solid-js";
import { TextCounter, countsOf, projectCounts, type TextCounts } from "../../editor/document-counts";
import type { FileOpenMode } from "../../types/buffer";
import type { EditorStore } from "./editor-store";

/** The counts of the document in the view. */
export interface DocumentCounts extends TextCounts {
  /** False while a large file is still being counted: the numbers are projected from the part counted so far. */
  exact: boolean;
}

/** The longest one slice of counting holds the main thread. */
export const COUNT_SLICE_MS = 6;

/**
 * How long a change waits for its recount. The changes inside the wait are
 * counted with it, so continuous typing is recounted at most once per wait.
 */
export const RECOUNT_DELAY_MS = 120;

export type CountsStore = ReturnType<typeof createCountsStore>;

function sameCounts(a: DocumentCounts | null, b: DocumentCounts | null): boolean {
  if (a === null || b === null) return a === b;
  return (
    a.words === b.words &&
    a.characters === b.characters &&
    a.cjk === b.cjk &&
    a.arabic === b.arabic &&
    a.exact === b.exact
  );
}

function projectsWhileCounting(mode: FileOpenMode | null): boolean {
  return mode?.kind === "LargeFile" || mode?.kind === "LargeFileConfirm";
}

/**
 * Word and character counts of the document in the view, counted only while
 * something on screen shows them, a slice at a time after the document
 * changes, never inside the keystroke that changed it.
 */
export function createCountsStore(editor: EditorStore) {
  const [counts, setCounts] = createSignal<DocumentCounts | null>(null, { equals: sameCounts });
  const [watchers, setWatchers] = createSignal(0);
  const counter = new TextCounter();
  let timer: ReturnType<typeof setTimeout> | null = null;
  let countedNote: string | null = null;

  function cancel(): void {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  }

  function schedule(delayMs: number): void {
    if (timer !== null) return;
    timer = setTimeout(countSlice, delayMs);
  }

  function countSlice(): void {
    timer = null;
    const doc = editor.liveDoc();
    if (doc === null) {
      setCounts(null);
      return;
    }
    const started = performance.now();
    const progress = counter.count(doc, () => performance.now() - started >= COUNT_SLICE_MS);
    if (progress.done) {
      setCounts({ ...countsOf(progress), exact: true });
      return;
    }
    if (projectsWhileCounting(editor.largeFileMode()) && progress.covered > 0) {
      setCounts({ ...projectCounts(progress, doc.length), exact: false });
    }
    schedule(0);
  }

  createEffect(
    on([watchers, editor.docVersion, editor.currentBufferId], ([watching, , note]) => {
      if (watching === 0) {
        cancel();
        return;
      }
      if (note !== countedNote) {
        countedNote = note;
        cancel();
        setCounts(null);
        schedule(0);
        return;
      }
      schedule(RECOUNT_DELAY_MS);
    }),
  );

  /** Keeps the counts current until the returned function is called. */
  function watch(): () => void {
    setWatchers((count) => count + 1);
    let released = false;
    return () => {
      if (released) return;
      released = true;
      setWatchers((count) => count - 1);
    };
  }

  return { counts, watch };
}
