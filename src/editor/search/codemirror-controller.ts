import { StateEffect } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import {
  SearchQuery,
  setSearchQuery,
  getSearchQuery,
  findNext,
  findPrevious,
  replaceNext,
  replaceAll,
} from "@codemirror/search";
import { findMatches, findMatchesPlugin } from "./find-matches";
import type {
  EditorSearchController,
  MatchState,
  MatchTick,
  SearchTerm,
} from "./types";

const EMPTY_STATE: MatchState = { current: 0, total: 0, capped: false };

export function createCodeMirrorSearchController(
  getView: () => EditorView | null,
): EditorSearchController {
  function toQuery(term: SearchTerm): SearchQuery {
    return new SearchQuery({
      search: term.query,
      caseSensitive: term.caseSensitive,
      wholeWord: term.wholeWord,
      regexp: term.regexp,
      replace: term.replace,
    });
  }

  function setQuery(term: SearchTerm) {
    const view = getView();
    if (!view) return;
    const query = toQuery(term.query.trim().length > 0 ? term : { ...term, query: "" });
    // The count lives in a plugin the editor installs; a view built without it
    // gets it with the first query, the way @codemirror/search adds its own
    // state on first use.
    const effects: StateEffect<unknown>[] = [setSearchQuery.of(query)];
    if (!view.plugin(findMatchesPlugin)) effects.push(StateEffect.appendConfig.of(findMatches()));
    view.dispatch({ effects });
  }

  // CodeMirror's commands open their own panel for a query that does not
  // compile, and walk the whole document for one that matches nothing. Neither
  // is a move Find makes.
  function canMove(view: EditorView): boolean {
    if (!getSearchQuery(view.state).valid) return false;
    return !view.plugin(findMatchesPlugin)?.foundNothing();
  }

  function next() {
    const view = getView();
    if (view && canMove(view)) findNext(view);
  }

  function previous() {
    const view = getView();
    if (view && canMove(view)) findPrevious(view);
  }

  function replaceCurrent() {
    const view = getView();
    if (view && canMove(view)) replaceNext(view);
  }

  function replaceEvery() {
    const view = getView();
    if (view && canMove(view)) replaceAll(view);
  }

  function matchState(): MatchState {
    const view = getView();
    return view?.plugin(findMatchesPlugin)?.matchState() ?? EMPTY_STATE;
  }

  function matchPositions(limit: number): MatchTick[] {
    const view = getView();
    return view?.plugin(findMatchesPlugin)?.ticks(limit) ?? [];
  }

  function clear() {
    const view = getView();
    if (!view) return;
    view.dispatch({ effects: setSearchQuery.of(new SearchQuery({ search: "" })) });
  }

  return {
    setQuery,
    next,
    previous,
    replaceCurrent,
    replaceAll: replaceEvery,
    matchState,
    matchPositions,
    clear,
  };
}
