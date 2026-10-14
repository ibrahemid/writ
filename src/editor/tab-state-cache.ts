import type { EditorState, StateEffect } from "@codemirror/state";
import type { FileOpenMode } from "../types/buffer";

/** An open tab's editor, kept while another tab is in front. */
export interface TabState {
  /** The state as the tab left it: document, selection, undo history. */
  state: EditorState;
  /** `EditorView.scrollSnapshot()` taken as the tab left. */
  scroll: StateEffect<unknown> | null;
  mode: FileOpenMode;
  /** The longest line the tab was loaded with ([`featuresFor`]). */
  longest: number;
}

/**
 * How much document text, in UTF-16 code units, the kept states may hold
 * together: about 128 MiB of strings at two bytes a unit. A webview has no
 * memory-pressure signal Writ can read (WKWebView exposes none), so the
 * budget stands in for one.
 */
export const TAB_STATE_BUDGET = 64 * 1024 * 1024;

function isLargeTier(mode: FileOpenMode): boolean {
  return mode.kind === "LargeFile" || mode.kind === "LargeFileConfirm";
}

/**
 * The editor states of the open tabs that are not in front, least recently
 * left first.
 *
 * Over the budget, the least recently left large-tier tab goes first, then
 * the least recently left of the rest. A tab whose document alone is over
 * the budget is not kept. `onEvict` hears every tab that is dropped to stay
 * within the budget.
 */
export class TabStateCache {
  private readonly entries = new Map<string, TabState>();
  private held = 0;

  constructor(
    private readonly budget = TAB_STATE_BUDGET,
    private readonly onEvict: (id: string) => void = () => {},
  ) {}

  /** Keeps `entry` for tab `id`, replacing what was kept for it. */
  put(id: string, entry: TabState): void {
    this.delete(id);
    const size = entry.state.doc.length;
    if (size > this.budget) {
      this.onEvict(id);
      return;
    }
    this.entries.set(id, entry);
    this.held += size;
    this.evictOver(id);
  }

  /** The state kept for tab `id`, taken out of the cache. */
  take(id: string): TabState | undefined {
    const entry = this.entries.get(id);
    if (entry !== undefined) this.delete(id);
    return entry;
  }

  peek(id: string): TabState | undefined {
    return this.entries.get(id);
  }

  /** Replaces the state kept for tab `id`, keeping its place in the order. */
  update(id: string, state: EditorState): void {
    const entry = this.entries.get(id);
    if (entry === undefined) return;
    this.held += state.doc.length - entry.state.doc.length;
    entry.state = state;
    entry.scroll = null;
    this.evictOver(id);
  }

  delete(id: string): void {
    const entry = this.entries.get(id);
    if (entry === undefined) return;
    this.entries.delete(id);
    this.held -= entry.state.doc.length;
  }

  /** Drops every tab that is not in `open`. */
  retain(open: ReadonlySet<string>): void {
    for (const id of [...this.entries.keys()]) {
      if (!open.has(id)) this.delete(id);
    }
  }

  clear(): void {
    this.entries.clear();
    this.held = 0;
  }

  /** The code units the kept documents hold together. */
  get size(): number {
    return this.held;
  }

  has(id: string): boolean {
    return this.entries.has(id);
  }

  private evictOver(keep: string): void {
    for (const largeFirst of [true, false]) {
      for (const [id, entry] of this.entries) {
        if (this.held <= this.budget) return;
        if (id === keep || isLargeTier(entry.mode) !== largeFirst) continue;
        this.delete(id);
        this.onEvict(id);
      }
    }
  }
}
