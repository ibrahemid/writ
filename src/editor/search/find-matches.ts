import { Facet, RangeSetBuilder, type EditorState, type Extension, type Range } from "@codemirror/state";
import {
  Decoration,
  ViewPlugin,
  type DecorationSet,
  type EditorView,
  type ViewUpdate,
} from "@codemirror/view";
import { getSearchQuery, searchPanelOpen, type SearchQuery } from "@codemirror/search";
import { MatchIndex } from "./match-index";
import { createQueryScanner, unquoteSearch, wordCharsKey } from "./scanner";
import type { MatchState, MatchTick } from "./types";

export const SYNC_SCAN_MAX_CHARS = 1024 * 1024;
const SLICE_MS = 6;
// The margin CodeMirror's own highlighter reads around the visible ranges.
const REGEXP_MARGIN = 250;

export interface FindMatchesConfig {
  onChange?: () => void;
  schedule?: (slice: () => void) => () => void;
  /** Runs `work` once the frame being drawn is on screen; returns a cancel. */
  afterFrame?: (work: () => void) => () => void;
}

// A MessageChannel task runs right after the current one, without the timer clamp.
function scheduleMessage(slice: () => void): () => void {
  const channel = new MessageChannel();
  let cancelled = false;
  channel.port1.onmessage = () => {
    channel.port1.close();
    if (!cancelled) slice();
  };
  channel.port2.postMessage(null);
  return () => {
    cancelled = true;
    channel.port1.close();
  };
}

// A timer set from a frame callback runs once that frame is drawn.
function scheduleAfterFrame(work: () => void): () => void {
  let timer: ReturnType<typeof setTimeout> | null = null;
  const frame = requestAnimationFrame(() => {
    timer = setTimeout(work, 0);
  });
  return () => {
    cancelAnimationFrame(frame);
    if (timer !== null) clearTimeout(timer);
  };
}

const findMatchesConfig = Facet.define<FindMatchesConfig, Required<FindMatchesConfig>>({
  combine(configs) {
    return {
      onChange: configs.find((config) => config.onChange)?.onChange ?? (() => {}),
      schedule: configs.find((config) => config.schedule)?.schedule ?? scheduleMessage,
      afterFrame: configs.find((config) => config.afterFrame)?.afterFrame ?? scheduleAfterFrame,
    };
  },
});

/** Whether two queries find the same matches; the replacement does not count. */
function sameMatches(a: SearchQuery, b: SearchQuery): boolean {
  return (
    a.search === b.search &&
    a.caseSensitive === b.caseSensitive &&
    a.regexp === b.regexp &&
    a.wholeWord === b.wholeWord &&
    a.literal === b.literal &&
    a.test === b.test
  );
}

function isInvalid(query: SearchQuery): boolean {
  return query.regexp && query.search.length > 0 && !query.valid;
}

class FindMatches {
  private query: SearchQuery;
  private index: MatchIndex | null = null;
  private wordChars: string | null = null;
  private cancelSlice: (() => void) | null = null;
  private cancelAfterEdit: (() => void) | null = null;
  private changeQueued = false;
  private destroyed = false;

  constructor(private readonly view: EditorView) {
    this.query = getSearchQuery(view.state);
    this.start(view.state);
  }

  update(update: ViewUpdate): void {
    const query = getSearchQuery(update.state);
    if (!sameMatches(query, this.query)) {
      this.query = query;
      this.start(update.state);
      this.changed();
      return;
    }
    if (!this.index) return;
    if (
      this.wordChars !== null &&
      (update.docChanged || update.selectionSet || update.transactions.some((tr) => tr.reconfigured)) &&
      wordCharsKey(update.state) !== this.wordChars
    ) {
      this.start(update.state);
      this.changed();
      return;
    }
    if (update.docChanged) {
      this.index.applyChanges(update.changes, update.startState.doc, update.state);
      this.followEdit(update.state);
    }
  }

  destroy(): void {
    this.destroyed = true;
    this.cancelSlice?.();
    this.cancelSlice = null;
    this.cancelAfterEdit?.();
    this.cancelAfterEdit = null;
  }

  private start(state: EditorState): void {
    this.cancelSlice?.();
    this.cancelSlice = null;
    this.cancelAfterEdit?.();
    this.cancelAfterEdit = null;
    const scanner = createQueryScanner(this.query);
    this.index = scanner ? new MatchIndex(scanner) : null;
    this.wordChars = scanner && this.query.wholeWord ? wordCharsKey(state) : null;
    this.read(state);
  }

  private read(state: EditorState): void {
    const index = this.index;
    if (!index || index.settled) return;
    if (state.doc.length <= SYNC_SCAN_MAX_CHARS) {
      index.advance(state, Infinity);
      return;
    }
    if (!this.cancelSlice) this.cancelSlice = this.config().schedule(this.slice);
  }

  /**
   * Reads the edited part of a document up to 1 MiB in the update, and of a
   * longer one once the frame the edit draws is on screen; the listener
   * hears then, once for all the edits that frame draws.
   */
  private followEdit(state: EditorState): void {
    if (state.doc.length <= SYNC_SCAN_MAX_CHARS) {
      this.read(state);
    } else {
      this.cancelSlice?.();
      this.cancelSlice = null;
    }
    this.cancelAfterEdit ??= this.config().afterFrame(this.afterEdit);
  }

  private afterEdit = (): void => {
    this.cancelAfterEdit = null;
    const index = this.index;
    if (this.destroyed || !index) return;
    if (!index.settled && !this.cancelSlice) this.slice();
    else this.config().onChange();
  };

  private slice = (): void => {
    this.cancelSlice = null;
    const index = this.index;
    if (this.destroyed || !index) return;
    index.advance(this.view.state, performance.now() + SLICE_MS);
    if (!index.settled) this.cancelSlice = this.config().schedule(this.slice);
    this.config().onChange();
  };

  private config(): Required<FindMatchesConfig> {
    return this.view.state.facet(findMatchesConfig);
  }

  /** Tells the listener after the update in progress has finished. */
  private changed(): void {
    if (this.changeQueued) return;
    this.changeQueued = true;
    queueMicrotask(() => {
      this.changeQueued = false;
      if (!this.destroyed) this.config().onChange();
    });
  }

  matchState(): MatchState {
    if (!this.index) {
      return { current: 0, total: 0, capped: false, pending: false, invalid: isInvalid(this.query) };
    }
    return this.index.matchState(this.view.state.selection.main);
  }

  ticks(limit: number): MatchTick[] {
    return this.index ? this.index.ticks(limit, this.view.state.doc.length) : [];
  }

  foundNothing(): boolean {
    return !this.index || this.index.failed || (this.index.settled && this.index.count === 0);
  }
}

export const findMatchesPlugin = ViewPlugin.fromClass(FindMatches);

const matchMark = Decoration.mark({ class: "cm-searchMatch" });
const selectedMatchMark = Decoration.mark({ class: "cm-searchMatch cm-searchMatch-selected" });

/** A stretch the highlighter searches: visible ranges whose margins overlap, joined, widened by the margin. */
interface Segment {
  from: number;
  end: number;
}

interface Highlights {
  decorations: DecorationSet;
  segments: Segment[];
}

function marginOf(query: SearchQuery): number {
  return query.regexp ? REGEXP_MARGIN : unquoteSearch(query).length;
}

// Searches must not overlap, or marks are added out of order: ranges whose
// margins overlap are searched as one, and a search stops at its end, past
// which the cursor for a pattern that crosses lines reads on to the end of
// the line holding its start + 5,000 (into the next range on a long line).
function segmentsOf(view: EditorView, margin: number): Segment[] {
  const length = view.state.doc.length;
  const joinWithin = 2 * Math.max(margin, REGEXP_MARGIN);
  const ranges = view.visibleRanges;
  const segments: Segment[] = [];
  for (let i = 0; i < ranges.length; i++) {
    const from = ranges[i].from;
    let to = ranges[i].to;
    while (i < ranges.length - 1 && to > ranges[i + 1].from - joinWithin) to = ranges[++i].to;
    segments.push({ from: Math.max(0, from - margin), end: Math.min(to + margin, length) });
  }
  return segments;
}

function addMatches(
  state: EditorState,
  query: SearchQuery,
  from: number,
  end: number,
  add: (from: number, to: number, mark: Decoration) => void,
): void {
  const cursor = query.getCursor(state, from, end);
  for (let step = cursor.next(); !step.done; step = cursor.next()) {
    const match = step.value;
    if (match.from >= end) break;
    const selected = state.selection.ranges.some((range) => range.from === match.from && range.to === match.to);
    add(match.from, match.to, selected ? selectedMatchMark : matchMark);
  }
}

// CodeMirror's search highlighter paints only while its own panel is open, and
// Find never opens it. This paints the same marks over the visible ranges
// with the same cursors.
function highlightVisible(view: EditorView): Highlights {
  const state = view.state;
  const query = getSearchQuery(state);
  if (!query.valid || searchPanelOpen(state)) return { decorations: Decoration.none, segments: [] };
  const segments = segmentsOf(view, marginOf(query));
  const builder = new RangeSetBuilder<Decoration>();
  try {
    for (const segment of segments) {
      addMatches(state, query, segment.from, segment.end, (from, to, mark) => builder.add(from, to, mark));
    }
  } catch (error) {
    // The library's regexp cursor reads on past `to` to the end of the line
    // holding `from` + 5,000, where V8 can give up on a long line.
    if (!(error instanceof RangeError)) throw error;
  }
  return { decorations: builder.finish(), segments };
}

function hasSelectedText(state: EditorState): boolean {
  return state.selection.ranges.some((range) => !range.empty);
}

/**
 * The marks after an edit, searched again only over the lines the edit
 * touched, for a literal query whose matches stay on one line; null when
 * the whole visible text has to be searched again.
 */
function highlightEdited(update: ViewUpdate, previous: Highlights): Highlights | null {
  const state = update.state;
  const query = getSearchQuery(state);
  if (query.regexp || !query.valid || searchPanelOpen(state)) return null;
  const search = unquoteSearch(query);
  if (search.includes("\n") || hasSelectedText(update.startState) || hasSelectedText(state)) return null;
  const segments = segmentsOf(update.view, search.length);
  const { changes } = update;
  if (
    segments.length !== previous.segments.length ||
    segments.some(
      (segment, i) =>
        changes.mapPos(previous.segments[i].from, -1) !== segment.from ||
        changes.mapPos(previous.segments[i].end, 1) !== segment.end,
    )
  ) {
    return null;
  }
  const doc = state.doc;
  const touched: { from: number; to: number }[] = [];
  changes.iterChangedRanges((_fromA, _toA, fromB, toB) => {
    const from = doc.lineAt(fromB).from;
    const to = doc.lineAt(toB).to;
    const last = touched[touched.length - 1];
    if (last && from <= last.to) last.to = Math.max(last.to, to);
    else touched.push({ from, to });
  });
  let decorations = previous.decorations.map(changes);
  for (const lines of touched) {
    for (const segment of segments) {
      const from = Math.max(segment.from, lines.from);
      const end = Math.min(segment.end, lines.to);
      if (from > end) continue;
      const add: Range<Decoration>[] = [];
      addMatches(state, query, from, end, (matchFrom, matchTo, mark) => add.push(mark.range(matchFrom, matchTo)));
      decorations = decorations.update({
        filterFrom: from,
        filterTo: end,
        filter: (markFrom) => markFrom < from || markFrom >= end,
        add,
      });
    }
  }
  return { decorations, segments };
}

const findHighlighter = ViewPlugin.fromClass(
  class {
    highlights: Highlights;

    constructor(view: EditorView) {
      this.highlights = highlightVisible(view);
    }

    update(update: ViewUpdate): void {
      const queryChanged = getSearchQuery(update.state) !== getSearchQuery(update.startState);
      if (update.docChanged && !queryChanged) {
        this.highlights = highlightEdited(update, this.highlights) ?? highlightVisible(update.view);
      } else if (update.docChanged || update.selectionSet || update.viewportChanged || queryChanged) {
        this.highlights = highlightVisible(update.view);
      }
    }
  },
  { decorations: (plugin) => plugin.highlights.decorations },
);

/**
 * Find's count, match map and highlights over CodeMirror's search state. Goes
 * alongside `search()`, which holds the query; `config.onChange` hears when
 * the count may have moved, including from a document still being read.
 */
export function findMatches(config: FindMatchesConfig = {}): Extension {
  return [findMatchesConfig.of(config), findMatchesPlugin, findHighlighter];
}
