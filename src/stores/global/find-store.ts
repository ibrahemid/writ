import { createSignal, createRoot } from "solid-js";
import type { EditorView } from "@codemirror/view";
import { windowRegistry } from "./window-registry";
import { createCodeMirrorSearchController } from "../../editor/search/codemirror-controller";
import type { MatchState, MatchTick, SearchTerm } from "../../editor/search/types";

const MAX_TICKS = 200;
const EMPTY_MATCH: MatchState = { current: 0, total: 0, capped: false };

export function createFindController(getView: () => EditorView | null) {
  const controller = createCodeMirrorSearchController(getView);

  const [isOpen, setIsOpen] = createSignal(false);
  const [queryText, setQueryTextSignal] = createSignal("");
  const [replaceText, setReplaceTextSignal] = createSignal("");
  const [replaceOpen, setReplaceOpen] = createSignal(false);
  const [caseSensitive, setCaseSensitive] = createSignal(false);
  const [wholeWord, setWholeWord] = createSignal(false);
  const [regexp, setRegexp] = createSignal(false);
  const [matches, setMatches] = createSignal<MatchState>(EMPTY_MATCH);
  const [ticks, setTicks] = createSignal<MatchTick[]>([]);
  const [focusNonce, setFocusNonce] = createSignal(0);

  function currentTerm(): SearchTerm {
    return {
      query: queryText(),
      caseSensitive: caseSensitive(),
      wholeWord: wholeWord(),
      regexp: regexp(),
      replace: replaceText(),
    };
  }

  function refresh() {
    setMatches(controller.matchState());
    setTicks(controller.matchPositions(MAX_TICKS));
  }

  function apply() {
    controller.setQuery(currentTerm());
    refresh();
  }

  function hasQuery() {
    return queryText().trim().length > 0;
  }

  function selectionSeed(): string {
    const view = getView();
    if (!view) return "";
    const main = view.state.selection.main;
    if (main.empty) return "";
    const text = view.state.doc.sliceString(main.from, main.to);
    return text.includes("\n") ? "" : text;
  }

  function open() {
    // Seed from the current selection on every invocation, not only the first,
    // so selecting new text and pressing find/replace replaces the query. With
    // no selection the previous query is preserved.
    const seed = selectionSeed();
    if (seed) setQueryTextSignal(seed);
    setIsOpen(true);
    apply();
    setFocusNonce((n) => n + 1);
  }

  function close() {
    if (!isOpen()) return;
    setIsOpen(false);
    controller.clear();
    getView()?.focus();
  }

  function setQueryText(value: string) {
    setQueryTextSignal(value);
    apply();
  }

  function setReplaceText(value: string) {
    setReplaceTextSignal(value);
  }

  function toggleReplace() {
    setReplaceOpen((v) => !v);
  }

  function showReplace() {
    open();
    setReplaceOpen(true);
  }

  function toggleCaseSensitive() {
    setCaseSensitive((v) => !v);
    apply();
  }

  function toggleWholeWord() {
    setWholeWord((v) => !v);
    apply();
  }

  function toggleRegexp() {
    setRegexp((v) => !v);
    apply();
  }

  function next() {
    if (!hasQuery()) return;
    controller.setQuery(currentTerm());
    controller.next();
    refresh();
  }

  function previous() {
    if (!hasQuery()) return;
    controller.setQuery(currentTerm());
    controller.previous();
    refresh();
  }

  function replaceCurrent() {
    if (!hasQuery()) return;
    controller.setQuery(currentTerm());
    controller.replaceCurrent();
    refresh();
  }

  function replaceAll() {
    if (!hasQuery()) return;
    controller.setQuery(currentTerm());
    controller.replaceAll();
    refresh();
  }

  function findNextCmd() {
    if (!isOpen()) open();
    next();
  }

  function findPrevCmd() {
    if (!isOpen()) open();
    previous();
  }

  return {
    isOpen,
    queryText,
    replaceText,
    replaceOpen,
    caseSensitive,
    wholeWord,
    regexp,
    matches,
    ticks,
    focusNonce,
    open,
    close,
    setQueryText,
    setReplaceText,
    toggleReplace,
    showReplace,
    toggleCaseSensitive,
    toggleWholeWord,
    toggleRegexp,
    next,
    previous,
    replaceCurrent,
    replaceAll,
    findNextCmd,
    findPrevCmd,
    refresh,
  };
}

export type FindController = ReturnType<typeof createFindController>;

// Singleton state — Writ is single-window; editor find targets the active
// window's editor view.
export const findStore = createRoot(() =>
  createFindController(() => windowRegistry.getActive()?.editor.getView() ?? null),
);
