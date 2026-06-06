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
import { countMatches, MATCH_COUNT_CAP } from "./count";
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
    view.dispatch({ effects: setSearchQuery.of(toQuery(term)) });
  }

  function next() {
    const view = getView();
    if (view) findNext(view);
  }

  function previous() {
    const view = getView();
    if (view) findPrevious(view);
  }

  function replaceCurrent() {
    const view = getView();
    if (view) replaceNext(view);
  }

  function replaceEvery() {
    const view = getView();
    if (view) replaceAll(view);
  }

  function matchState(): MatchState {
    const view = getView();
    if (!view) return EMPTY_STATE;
    const query = getSearchQuery(view.state);
    return countMatches(view.state, query, MATCH_COUNT_CAP);
  }

  function matchPositions(limit: number): MatchTick[] {
    const view = getView();
    if (!view) return [];
    const query = getSearchQuery(view.state);
    if (!query.search || !query.valid) return [];

    const length = view.state.doc.length || 1;
    const ticks: MatchTick[] = [];
    const cursor = query.getCursor(view.state);

    let res = cursor.next();
    while (!res.done && ticks.length < limit) {
      ticks.push({ fraction: res.value.from / length });
      res = cursor.next();
    }
    return ticks;
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
