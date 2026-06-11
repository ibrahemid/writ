import type {
  EditorSearchController,
  MatchState,
  MatchTick,
  SearchTerm,
} from "./types";

// Search controller for the preview pane. Unlike the CodeMirror controller it
// is asynchronous: commands are posted to the cross-origin iframe runtime, and
// results arrive later via applyResult, which refreshes a synchronous snapshot
// the find overlay reads. Preview is read-only, so replace is a no-op.

/** A find outcome reported by the iframe runtime. */
export interface PreviewFindResult {
  current: number;
  total: number;
  capped: boolean;
  ticks: MatchTick[];
}

/** Command sent down to the iframe runtime. */
export type PreviewFindCommand =
  | { type: "find"; query: string; caseSensitive: boolean; wholeWord: boolean; regexp: boolean }
  | { type: "findNext" }
  | { type: "findPrev" }
  | { type: "findClear" };

export interface PreviewSearchDeps {
  /** Send a command to the iframe runtime. */
  post(command: PreviewFindCommand): void;
  /** Invoked after a result updates the snapshot so the overlay can refresh. */
  onUpdate(): void;
}

export interface PreviewSearchController extends EditorSearchController {
  /** Feed a runtime find result into the synchronous snapshot. */
  applyResult(result: PreviewFindResult): void;
  /** Re-post the active query (e.g. after the iframe reloads and lost its
   *  highlights). No-op when no query is active. */
  reapply(): void;
}

const EMPTY_STATE: MatchState = { current: 0, total: 0, capped: false };

export function createPreviewSearchController(
  deps: PreviewSearchDeps,
): PreviewSearchController {
  let state: MatchState = EMPTY_STATE;
  let ticks: MatchTick[] = [];
  let lastCommand: PreviewFindCommand | null = null;

  function setQuery(term: SearchTerm): void {
    if (term.query.trim().length === 0) {
      // Reset the snapshot immediately so the count clears without waiting for
      // the round-trip; still notify the runtime to drop any highlights.
      state = EMPTY_STATE;
      ticks = [];
    }
    const command: PreviewFindCommand = {
      type: "find",
      query: term.query,
      caseSensitive: term.caseSensitive,
      wholeWord: term.wholeWord,
      regexp: term.regexp,
    };
    lastCommand = command.query ? command : null;
    deps.post(command);
  }

  function reapply(): void {
    if (lastCommand) deps.post(lastCommand);
  }

  function next(): void {
    deps.post({ type: "findNext" });
  }

  function previous(): void {
    deps.post({ type: "findPrev" });
  }

  function replaceCurrent(): void {
    /* preview is read-only */
  }

  function replaceAll(): void {
    /* preview is read-only */
  }

  function matchState(): MatchState {
    return state;
  }

  function matchPositions(limit: number): MatchTick[] {
    return ticks.slice(0, limit);
  }

  function clear(): void {
    state = EMPTY_STATE;
    ticks = [];
    lastCommand = null;
    deps.post({ type: "findClear" });
  }

  function applyResult(result: PreviewFindResult): void {
    state = { current: result.current, total: result.total, capped: result.capped };
    ticks = result.ticks ?? [];
    deps.onUpdate();
  }

  return {
    setQuery,
    next,
    previous,
    replaceCurrent,
    replaceAll,
    matchState,
    matchPositions,
    clear,
    applyResult,
    reapply,
  };
}
