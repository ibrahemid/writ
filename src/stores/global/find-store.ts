import { createSignal, createRoot } from "solid-js";
import type { EditorView } from "@codemirror/view";
import { windowRegistry } from "./window-registry";
import { createCodeMirrorSearchController } from "../../editor/search/codemirror-controller";
import type { EditorSearchController, MatchState, MatchTick, SearchTerm } from "../../editor/search/types";

const MAX_TICKS = 200;
const EMPTY_MATCH: MatchState = { current: 0, total: 0, capped: false };

// A search surface is a search controller plus the bits find needs that differ
// between the editor and the preview: how to seed a query from the current
// selection, where to return focus on close, and whether replace is offered.
export interface SearchSurface extends EditorSearchController {
  selectionSeed(): string;
  focus(): void;
  canReplace(): boolean;
}

/** Wrap the active CodeMirror view as a search surface. */
export function createEditorSurface(getView: () => EditorView | null): SearchSurface {
  const controller = createCodeMirrorSearchController(getView);
  return {
    ...controller,
    selectionSeed() {
      const view = getView();
      if (!view) return "";
      const main = view.state.selection.main;
      if (main.empty) return "";
      const text = view.state.doc.sliceString(main.from, main.to);
      return text.includes("\n") ? "" : text;
    },
    focus() {
      getView()?.focus();
    },
    canReplace() {
      return true;
    },
  };
}

export function createFindController(getSurface: () => SearchSurface | null) {
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
  const [canReplaceSignal, setCanReplace] = createSignal(true);

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
    const surface = getSurface();
    setMatches(surface ? surface.matchState() : EMPTY_MATCH);
    setTicks(surface ? surface.matchPositions(MAX_TICKS) : []);
    setCanReplace(surface ? surface.canReplace() : true);
  }

  function apply() {
    getSurface()?.setQuery(currentTerm());
    refresh();
  }

  function hasQuery() {
    return queryText().trim().length > 0;
  }

  function open() {
    // Seed from the current selection on every invocation, not only the first,
    // so selecting new text and pressing find/replace replaces the query. With
    // no selection the previous query is preserved.
    const seed = getSurface()?.selectionSeed() ?? "";
    if (seed) setQueryTextSignal(seed);
    setIsOpen(true);
    apply();
    setFocusNonce((n) => n + 1);
  }

  function close() {
    if (!isOpen()) return;
    setIsOpen(false);
    getSurface()?.clear();
    getSurface()?.focus();
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
    const surface = getSurface();
    if (!surface) return;
    surface.setQuery(currentTerm());
    surface.next();
    refresh();
  }

  function previous() {
    if (!hasQuery()) return;
    const surface = getSurface();
    if (!surface) return;
    surface.setQuery(currentTerm());
    surface.previous();
    refresh();
  }

  function replaceCurrent() {
    if (!hasQuery()) return;
    const surface = getSurface();
    if (!surface) return;
    surface.setQuery(currentTerm());
    surface.replaceCurrent();
    refresh();
  }

  function replaceAll() {
    if (!hasQuery()) return;
    const surface = getSurface();
    if (!surface) return;
    surface.setQuery(currentTerm());
    surface.replaceAll();
    refresh();
  }

  // Re-apply the current query to whatever surface is now active. Called when
  // the find target changes under an open overlay (e.g. a layout flip into or
  // out of preview-only) so the newly-active surface highlights and counts
  // immediately instead of waiting for the next keystroke.
  function retarget() {
    if (isOpen()) apply();
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
    canReplace: canReplaceSignal,
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
    retarget,
  };
}

export type FindController = ReturnType<typeof createFindController>;

// Singleton state — Writ is single-window. Find targets the preview when a
// renderable buffer is shown preview-only (the editor is hidden), otherwise the
// active editor view. The preview pane registers/unregisters its search
// controller as that condition flips.
export const findStore = createRoot(() =>
  createFindController(() => {
    const win = windowRegistry.getActive();
    if (!win) return null;
    const preview = win.preview.activeSearch();
    if (preview) {
      return {
        ...preview,
        selectionSeed: () => "",
        focus: () => {},
        canReplace: () => false,
      };
    }
    return createEditorSurface(() => win.editor.getView() ?? null);
  }),
);
