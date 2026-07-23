import { StateField, StateEffect, RangeSetBuilder, type Extension, type EditorState } from "@codemirror/state";
import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";
import type { SpellingLint } from "../types/spelling";
import "../components/Editor/cm-spelling.css";

// Spell-check decorations live in a StateField so they map through document
// edits automatically (RangeSet mapping), and so "Fix all" and the preview
// always read the CURRENT, remapped ranges rather than stale request offsets.
// New results arrive as a StateEffect; the field rebuilds from them.

/** One flagged range, resolved against the live document. */
export interface SpellingEntry {
  from: number;
  to: number;
  /** Document text at [from, to) when the decoration was built. */
  word: string;
  message: string;
  kind: string;
  suggestions: string[];
  confident: boolean;
}

/** A single replacement, in current-document coordinates. */
export interface SpellingFix {
  from: number;
  to: number;
  insert: string;
}

type SpellingData = Omit<SpellingEntry, "from" | "to">;

/** Replaces the decoration set with marks built from a fresh lint result. */
export const setSpellingLints = StateEffect.define<SpellingLint[]>();
/** Clears all spelling decorations. */
export const clearSpellingLints = StateEffect.define<void>();
/** Drops every decoration whose flagged word equals the payload. */
export const removeSpellingWord = StateEffect.define<string>();

function markFor(word: string, lint: SpellingLint): Decoration {
  const data: SpellingData = {
    word,
    message: lint.message,
    kind: lint.kind,
    suggestions: lint.suggestions,
    confident: lint.confident,
  };
  return Decoration.mark({
    class: "cm-spelling-error",
    attributes: { title: lint.message },
    // Extra spec fields ride along on the decoration and survive mapping.
    spelling: data,
  } as Parameters<typeof Decoration.mark>[0]);
}

function readData(value: Decoration): SpellingData | null {
  const spec = value.spec as { spelling?: SpellingData };
  return spec.spelling ?? null;
}

function buildDecorations(state: EditorState, lints: SpellingLint[]): DecorationSet {
  const docLen = state.doc.length;
  const builder = new RangeSetBuilder<Decoration>();
  let lastTo = -1;
  for (const lint of lints) {
    const from = Math.min(Math.max(lint.fromUtf16, 0), docLen);
    const to = Math.min(Math.max(lint.toUtf16, 0), docLen);
    if (from >= to) continue;
    // RangeSetBuilder demands sorted, non-overlapping additions. Rust sorts by
    // start; drop the rare overlap (a spelling word inside a mechanical span).
    if (from < lastTo) continue;
    const word = state.doc.sliceString(from, to);
    builder.add(from, to, markFor(word, lint));
    lastTo = to;
  }
  return builder.finish();
}

export const spellingField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(deco, tr) {
    // Map first so surviving decorations track edits, then apply effects.
    deco = deco.map(tr.changes);
    // Drop any decoration whose text was touched by this edit: a corrected
    // word must not keep its underline (nor its count) until the next re-lint.
    // Untouched ranges elsewhere in the document are left mapped.
    if (tr.docChanged) {
      const changed: Array<[number, number]> = [];
      tr.changes.iterChangedRanges((_fromA, _toA, fromB, toB) => changed.push([fromB, toB]));
      if (changed.length > 0) {
        deco = deco.update({
          filter: (from, to) => !changed.some(([a, b]) => from < b && to > a),
        });
      }
    }
    for (const effect of tr.effects) {
      if (effect.is(setSpellingLints)) {
        deco = buildDecorations(tr.state, effect.value);
      } else if (effect.is(clearSpellingLints)) {
        deco = Decoration.none;
      } else if (effect.is(removeSpellingWord)) {
        const word = effect.value;
        deco = deco.update({
          filter: (_from, _to, value) => readData(value)?.word !== word,
        });
      }
    }
    return deco;
  },
  provide: (f) => EditorView.decorations.from(f),
});

/**
 * The spell-check extension. `onCountChange` is called with the live number of
 * decorations whenever the set changes (new results, edits, fixes), so the
 * status-bar chip never shows a stale count.
 */
export function spellingExtension(onCountChange: (count: number) => void): Extension {
  return [
    spellingField,
    EditorView.updateListener.of((update) => {
      const before = update.startState.field(spellingField, false);
      const after = update.state.field(spellingField, false);
      if (before !== after) {
        onCountChange(after ? after.size : 0);
      }
    }),
  ];
}

/** Reads the current, remapped decorations as plain entries. */
export function spellingEntries(state: EditorState): SpellingEntry[] {
  const set = state.field(spellingField, false);
  if (!set) return [];
  const out: SpellingEntry[] = [];
  const cursor = set.iter();
  while (cursor.value) {
    const data = readData(cursor.value);
    if (data) {
      out.push({ from: cursor.from, to: cursor.to, ...data });
    }
    cursor.next();
  }
  return out;
}

/**
 * Builds replacements from mapped entries. Pure: takes a `docSlice` reader so it
 * can be unit-tested without a live editor. Drops any entry that is not
 * confident, offers no suggestion, or whose current text no longer equals the
 * flagged word — never applies from stale offsets.
 */
export function computeFixChanges(
  entries: SpellingEntry[],
  docSlice: (from: number, to: number) => string,
  accept?: (entry: SpellingEntry) => boolean,
): SpellingFix[] {
  const fixes: SpellingFix[] = [];
  for (const entry of entries) {
    if (!entry.confident) continue;
    if (accept && !accept(entry)) continue;
    const insert = entry.suggestions[0];
    if (insert === undefined) continue;
    if (docSlice(entry.from, entry.to) !== entry.word) continue;
    fixes.push({ from: entry.from, to: entry.to, insert });
  }
  return fixes;
}

/** Applies fixes in a single transaction — one undo step. */
export function applySpellingFixes(view: EditorView, fixes: SpellingFix[]): number {
  if (fixes.length === 0) return 0;
  view.dispatch({ changes: fixes, userEvent: "input.spelling.fix" });
  return fixes.length;
}
