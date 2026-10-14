import {
  StateField,
  StateEffect,
  RangeSetBuilder,
  type ChangeDesc,
  type Extension,
  type EditorState,
  type Range,
} from "@codemirror/state";
import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";
import { syntaxTree } from "@codemirror/language";
import type { SpellingLint } from "../types/spelling";
import type { SpellingConfig } from "../types/config";
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

/** A range of the document and the lints found in it, in document positions. */
export interface CheckedRange {
  from: number;
  to: number;
  lints: SpellingLint[];
}

/**
 * Replaces the marks inside each checked range with the lints found there,
 * and leaves every mark outside them where it is.
 */
export const setSpellingLintsIn = StateEffect.define<CheckedRange[]>();
/** Clears all spelling decorations. */
export const clearSpellingLints = StateEffect.define<void>();
/** Marks the whole document unchecked, for a change to what the checker reads. */
export const recheckSpelling = StateEffect.define<void>();
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

function marksFor(state: EditorState, lints: SpellingLint[]): Range<Decoration>[] {
  const docLen = state.doc.length;
  const out: Range<Decoration>[] = [];
  for (const lint of lints) {
    const from = Math.min(Math.max(lint.fromUtf16, 0), docLen);
    const to = Math.min(Math.max(lint.toUtf16, 0), docLen);
    if (from >= to) continue;
    out.push(markFor(state.doc.sliceString(from, to), lint).range(from, to));
  }
  return out;
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
      const changed = changedRanges(tr.changes);
      if (changed.length > 0) {
        deco = deco.update({ filter: (from, to) => !overlapsAny(changed, from, to) });
      }
    }
    for (const effect of tr.effects) {
      if (effect.is(setSpellingLints)) {
        deco = buildDecorations(tr.state, effect.value);
      } else if (effect.is(setSpellingLintsIn)) {
        if (effect.value.length === 0) continue;
        const checked = [...effect.value].sort((a, b) => a.from - b.from);
        const spans = joinSorted(checked);
        deco = deco.update({
          filter: (from, to) => !overlapsAny(spans, from, to),
          add: checked.flatMap((range) => marksFor(tr.state, range.lints)),
          sort: true,
        });
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

/** A stretch of the document edited since it was last checked. */
export interface PendingRange {
  from: number;
  to: number;
}

/**
 * The most stretches one check sends. Edits that leave more than this pending
 * (a Replace All, typing with many cursors) are held as one stretch from the
 * first of them to the last, so the bookkeeping, the requests and the landing
 * of their results stay bounded by this number, not by the number of changes.
 */
export const MAX_CHECKED_STRETCHES = 16;

// What a change touched, in the new document's positions: sorted and disjoint.
function changedRanges(changes: ChangeDesc): PendingRange[] {
  const out: PendingRange[] = [];
  changes.iterChangedRanges((_fromA, _toA, fromB, toB) => out.push({ from: fromB, to: toB }));
  return out;
}

// Joins stretches sorted by start into the fewest disjoint ones. Stretches
// that touch are joined.
function joinSorted(sorted: readonly PendingRange[]): PendingRange[] {
  const out: PendingRange[] = [];
  for (const range of sorted) {
    const last = out[out.length - 1];
    if (last === undefined || range.from > last.to) out.push({ from: range.from, to: range.to });
    else if (range.to > last.to) last.to = range.to;
  }
  return out;
}

// Merges two lists sorted by start into one list sorted by start.
function mergeByStart(a: readonly PendingRange[], b: readonly PendingRange[]): PendingRange[] {
  const out: PendingRange[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length || j < b.length) {
    if (j >= b.length || (i < a.length && a[i].from <= b[j].from)) out.push(a[i++]);
    else out.push(b[j++]);
  }
  return out;
}

// Whether [from, to) overlaps one of `spans`, which are sorted and disjoint.
// Only the first span that ends after `from` can.
function overlapsAny(spans: readonly PendingRange[], from: number, to: number): boolean {
  let low = 0;
  let high = spans.length;
  while (low < high) {
    const middle = (low + high) >> 1;
    if (spans[middle].to > from) high = middle;
    else low = middle + 1;
  }
  return low < spans.length && spans[low].from < to;
}

// Disjoint stretches sorted by start, held to at most MAX_CHECKED_STRETCHES.
function held(ranges: PendingRange[]): readonly PendingRange[] {
  if (ranges.length <= MAX_CHECKED_STRETCHES) return ranges;
  return [{ from: ranges[0].from, to: ranges[ranges.length - 1].to }];
}

function mapPending(ranges: readonly PendingRange[], changes: ChangeDesc): PendingRange[] {
  const out: PendingRange[] = [];
  for (const range of ranges) {
    const from = changes.mapPos(range.from, -1);
    const to = Math.max(from, changes.mapPos(range.to, 1));
    out.push({ from, to });
  }
  return out;
}

/**
 * What has not been checked since it was written: the whole document when
 * the checker attaches or its rules change ([`recheckSpelling`]), then each
 * edit's range, every range mapped through the edits after it, at most
 * [`MAX_CHECKED_STRETCHES`] of them. A result that lands clears it.
 */
export const spellingPending = StateField.define<readonly PendingRange[]>({
  create: (state) => [{ from: 0, to: state.doc.length }],
  update(ranges, tr) {
    let next = ranges;
    if (tr.docChanged) {
      next = held(joinSorted(mergeByStart(mapPending(next, tr.changes), changedRanges(tr.changes))));
    }
    for (const effect of tr.effects) {
      if (effect.is(setSpellingLintsIn) || effect.is(setSpellingLints)) next = [];
      else if (effect.is(recheckSpelling)) next = [{ from: 0, to: tr.state.doc.length }];
    }
    return next;
  },
});

/** What the checker reads besides the text. */
export type SpellingRules = Pick<SpellingConfig, "dialect" | "ignored_words">;

/** Whether two sets of rules give the same verdicts on the same text. */
export function sameSpellingRules(a: SpellingRules, b: SpellingRules): boolean {
  return (
    a.dialect === b.dialect &&
    a.ignored_words.length === b.ignored_words.length &&
    a.ignored_words.every((word, i) => word === b.ignored_words[i])
  );
}

// How far a check reaches past an edit for the rest of its paragraph, in lines
// each way. A document with no blank lines is checked a bounded stretch at a
// time.
const PARAGRAPH_REACH_LINES = 32;

// Blocks a check must not cut through: the checker reads Markdown, and a slice that
// starts inside a fence would read the code in it as prose.
const WHOLE_BLOCKS = new Set(["FencedCode", "CodeBlock", "HTMLBlock", "CommentBlock"]);

function isBlank(text: string): boolean {
  return text.trim() === "";
}

function paragraphAround(state: EditorState, from: number, to: number): PendingRange {
  const doc = state.doc;
  let first = doc.lineAt(from);
  for (let reach = 0; reach < PARAGRAPH_REACH_LINES && first.number > 1; reach++) {
    const above = doc.line(first.number - 1);
    if (isBlank(above.text)) break;
    first = above;
  }
  let last = doc.lineAt(to);
  for (let reach = 0; reach < PARAGRAPH_REACH_LINES && last.number < doc.lines; reach++) {
    const below = doc.line(last.number + 1);
    if (isBlank(below.text)) break;
    last = below;
  }
  let start = first.from;
  let end = last.to;
  syntaxTree(state).iterate({
    from: start,
    to: end,
    enter: (node) => {
      if (!WHOLE_BLOCKS.has(node.name)) return;
      start = Math.min(start, node.from);
      end = Math.max(end, node.to);
      return false;
    },
  });
  return { from: doc.lineAt(start).from, to: doc.lineAt(end).to };
}

/**
 * The stretches a check sends: each pending range widened to its paragraph
 * and to any code block it touches, overlapping stretches merged.
 */
export function checkRanges(state: EditorState): PendingRange[] {
  const pending = state.field(spellingPending, false) ?? [];
  const widened = pending.map((range) => paragraphAround(state, range.from, range.to));
  return joinSorted(widened.sort((a, b) => a.from - b.from));
}

/**
 * The spell-check extension. `onCountChange` is called with the live number of
 * decorations whenever the set changes (new results, edits, fixes), so the
 * status-bar chip never shows a stale count.
 */
export function spellingExtension(onCountChange: (count: number) => void): Extension {
  return [
    spellingPending,
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

/**
 * Builds the replacement for one entry the user picked explicitly.
 *
 * Unlike [`computeFixChanges`] this ignores `confident` and the first-suggestion
 * rule: the user chose this word and this replacement, so the only guard that
 * still applies is the stale-offset check. Returns `null` when the text at the
 * entry's range is no longer the flagged word.
 */
export function computeChosenFix(
  entry: SpellingEntry,
  docSlice: (from: number, to: number) => string,
  replacement: string,
): SpellingFix | null {
  if (docSlice(entry.from, entry.to) !== entry.word) return null;
  return { from: entry.from, to: entry.to, insert: replacement };
}

/** Applies fixes in a single transaction — one undo step. */
export function applySpellingFixes(view: EditorView, fixes: SpellingFix[]): number {
  if (fixes.length === 0) return 0;
  view.dispatch({ changes: fixes, userEvent: "input.spelling.fix" });
  return fixes.length;
}
