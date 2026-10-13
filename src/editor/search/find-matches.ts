import { Facet, RangeSetBuilder, type EditorState, type Extension } from "@codemirror/state";
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

const findMatchesConfig = Facet.define<FindMatchesConfig, Required<FindMatchesConfig>>({
  combine(configs) {
    return {
      onChange: configs.find((config) => config.onChange)?.onChange ?? (() => {}),
      schedule: configs.find((config) => config.schedule)?.schedule ?? scheduleMessage,
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
      this.read(update.state);
      this.changed();
    }
  }

  destroy(): void {
    this.destroyed = true;
    this.cancelSlice?.();
    this.cancelSlice = null;
  }

  private start(state: EditorState): void {
    this.cancelSlice?.();
    this.cancelSlice = null;
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

// CodeMirror's search highlighter paints only while its own panel is open, and
// Find never opens it. This paints the same marks over the visible ranges
// with the same cursors.
function highlightVisible(view: EditorView): DecorationSet {
  const state = view.state;
  const query = getSearchQuery(state);
  if (!query.valid || searchPanelOpen(state)) return Decoration.none;
  const margin = query.regexp ? REGEXP_MARGIN : unquoteSearch(query).length;
  // Searches must not overlap, or marks are added out of order: ranges whose
  // margins overlap are searched as one, and a search stops at its end, past
  // which the cursor for a pattern that crosses lines reads on to the end of
  // the line holding its start + 5,000 (into the next range on a long line).
  const joinWithin = 2 * Math.max(margin, REGEXP_MARGIN);
  const builder = new RangeSetBuilder<Decoration>();
  const ranges = view.visibleRanges;
  try {
    for (let i = 0; i < ranges.length; i++) {
      const from = ranges[i].from;
      let to = ranges[i].to;
      while (i < ranges.length - 1 && to > ranges[i + 1].from - joinWithin) to = ranges[++i].to;
      const end = Math.min(to + margin, state.doc.length);
      const cursor = query.getCursor(state, Math.max(0, from - margin), end);
      for (let step = cursor.next(); !step.done; step = cursor.next()) {
        const match = step.value;
        if (match.from >= end) break;
        const selected = state.selection.ranges.some((range) => range.from === match.from && range.to === match.to);
        builder.add(match.from, match.to, selected ? selectedMatchMark : matchMark);
      }
    }
  } catch (error) {
    // The library's regexp cursor reads on past `to` to the end of the line
    // holding `from` + 5,000, where V8 can give up on a long line.
    if (!(error instanceof RangeError)) throw error;
  }
  return builder.finish();
}

const findHighlighter = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = highlightVisible(view);
    }

    update(update: ViewUpdate): void {
      if (
        update.docChanged ||
        update.selectionSet ||
        update.viewportChanged ||
        getSearchQuery(update.state) !== getSearchQuery(update.startState)
      ) {
        this.decorations = highlightVisible(update.view);
      }
    }
  },
  { decorations: (plugin) => plugin.decorations },
);

/**
 * Find's count, match map and highlights over CodeMirror's search state. Goes
 * alongside `search()`, which holds the query; `config.onChange` hears when
 * the count may have moved, including from a document still being read.
 */
export function findMatches(config: FindMatchesConfig = {}): Extension {
  return [findMatchesConfig.of(config), findMatchesPlugin, findHighlighter];
}
