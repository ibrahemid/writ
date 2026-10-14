import { createSignal, createRoot } from "solid-js";
import type { EditorView } from "@codemirror/view";
import { windowRegistry } from "./window-registry";
import { createCodeMirrorSearchController } from "../../editor/search/codemirror-controller";
import type { EditorSearchController, MatchState, MatchTick, SearchTerm } from "../../editor/search/types";

const MAX_TICKS = 200;
const EMPTY_MATCH: MatchState = { current: 0, total: 0, capped: false };
export const FIND_INPUT_DEBOUNCE_MS = 80;

function sameMatchState(a: MatchState, b: MatchState): boolean {
  return (
    a.current === b.current &&
    a.total === b.total &&
    a.capped === b.capped &&
    !!a.pending === !!b.pending &&
    !!a.invalid === !!b.invalid &&
    !!a.failed === !!b.failed
  );
}

// The map draws a tick at a percentage to three decimals; a tick that has not
// moved that far has not moved.
const tickPlace = (tick: MatchTick) => Math.round(tick.fraction * 1e5);

function sameTicks(a: MatchTick[], b: MatchTick[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (tickPlace(a[i]) !== tickPlace(b[i])) return false;
  return true;
}

export interface FindControllerOptions {
  /** Applies a query typed into the field this long after the last keystroke; 0 applies it at once. */
  inputDebounceMs?: number;
}

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

export function createFindController(
  getSurface: () => SearchSurface | null,
  options: FindControllerOptions = {},
) {
  const inputDebounceMs = options.inputDebounceMs ?? 0;
  let pendingApply: ReturnType<typeof setTimeout> | null = null;
  const [isOpen, setIsOpen] = createSignal(false);
  const [queryText, setQueryTextSignal] = createSignal("");
  // The text of the query the surface last received; `matches` describe it.
  const [appliedQuery, setAppliedQuery] = createSignal("");
  const [replaceText, setReplaceTextSignal] = createSignal("");
  const [replaceOpen, setReplaceOpen] = createSignal(false);
  const [caseSensitive, setCaseSensitive] = createSignal(false);
  const [wholeWord, setWholeWord] = createSignal(false);
  const [regexp, setRegexp] = createSignal(false);
  const [matches, setMatches] = createSignal<MatchState>(EMPTY_MATCH, { equals: sameMatchState });
  const [ticks, setTicks] = createSignal<MatchTick[]>([], { equals: sameTicks });
  const [focusNonce, setFocusNonce] = createSignal(0);
  const [canReplaceSignal, setCanReplace] = createSignal(true);
  // How many matches the last Replace all replaced, for the query it ran on.
  // A count that says "No results" right after a replace reads as a failure.
  const [replaced, setReplaced] = createSignal<{ query: string; count: number; more: boolean } | null>(
    null,
  );

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

  function cancelPendingApply() {
    if (pendingApply === null) return;
    clearTimeout(pendingApply);
    pendingApply = null;
  }

  function send(surface: SearchSurface | null) {
    cancelPendingApply();
    setAppliedQuery(queryText());
    surface?.setQuery(currentTerm());
  }

  function apply() {
    setReplaced(null);
    send(getSurface());
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
    cancelPendingApply();
    if (!isOpen()) return;
    setIsOpen(false);
    setAppliedQuery("");
    getSurface()?.clear();
    getSurface()?.focus();
  }

  function setQueryText(value: string) {
    setQueryTextSignal(value);
    if (inputDebounceMs <= 0) {
      apply();
      return;
    }
    cancelPendingApply();
    pendingApply = setTimeout(apply, inputDebounceMs);
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
    send(surface);
    surface.next();
    refresh();
  }

  function previous() {
    if (!hasQuery()) return;
    const surface = getSurface();
    if (!surface) return;
    send(surface);
    surface.previous();
    refresh();
  }

  function replaceCurrent() {
    if (!hasQuery()) return;
    const surface = getSurface();
    if (!surface) return;
    send(surface);
    surface.replaceCurrent();
    refresh();
  }

  function replaceAll() {
    if (!hasQuery()) return;
    const surface = getSurface();
    if (!surface) return;
    send(surface);
    const before = matches();
    surface.replaceAll();
    refresh();
    if (before.total > 0) {
      setReplaced({
        query: appliedQuery(),
        count: before.total,
        more: before.capped === true || before.pending === true,
      });
    }
  }

  /** What the last Replace all replaced, while the query it ran on stands. */
  function lastReplaced(): { count: number; more: boolean } | null {
    const last = replaced();
    if (last === null || last.query !== appliedQuery()) return null;
    return { count: last.count, more: last.more };
  }

  // Called when the find target changes (a layout flip into or out of
  // preview-only, a tab switch). An open overlay seeds the surface now in
  // front; a closed one clears it, since a surface Find left while open (the
  // editor hidden behind preview-only) still holds the query and its highlights.
  function retarget() {
    if (isOpen()) apply();
    else getSurface()?.clear();
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
    appliedQuery,
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
    lastReplaced,
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
  createFindController(
    () => {
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
    },
    { inputDebounceMs: FIND_INPUT_DEBOUNCE_MS },
  ),
);
