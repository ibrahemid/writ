import { EditorView } from "@codemirror/view";
import type { Extension } from "@codemirror/state";
import { findLinkTargets, type LinkRange } from "./link-layer";
import type { SpellingEntry } from "./spelling";
import { IS_MAC } from "../lib/platform";

/**
 * The editor's right-click menu.
 *
 * Split in two on purpose: [`buildEditorMenuItems`] is a pure function from a
 * description of what sits under the pointer to the list of rows, so the policy
 * (what appears when text is selected, when it is not, when the pointer is on a
 * misspelling or a link) is testable without a DOM; [`editorContextMenu`] is
 * the CodeMirror extension that gathers that description and opens the menu.
 *
 * Attached as a CM DOM handler rather than a document listener: it needs
 * `posAtCoords` to know what is under the pointer, and it keeps the editor out
 * of the app-wide listener path.
 */

/** One row, matching `ContextMenu`'s item shape. */
export interface EditorMenuItem {
  label: string;
  action: () => void;
  separator?: boolean;
  disabled?: boolean;
  danger?: boolean;
  kbd?: string;
}

/** Everything under the pointer that changes what the menu offers. */
export interface EditorMenuContext {
  /** A non-empty selection exists. */
  hasSelection: boolean;
  /** The selected text, for labelling the search row. */
  selectionText: string;
  /** The misspelling under the pointer, when there is one. */
  spelling: SpellingEntry | null;
  /** The link under the pointer, with its text resolved. */
  link: { range: LinkRange; text: string } | null;
  /** The rewrite feature is switched on. */
  aiEnabled: boolean;
  /** The document contains `{{placeholders}}`. */
  hasPlaceholders: boolean;
  /** The buffer accepts edits (false for a read-only or oversized buffer). */
  editable: boolean;
}

/** Actions the menu delegates to. Injected so this module stays free of stores
 * and services, mirroring how `link-layer` takes its openers. */
export interface EditorMenuActions {
  cut(): void;
  copy(): void;
  paste(): void;
  selectAll(): void;
  applySpelling(entry: SpellingEntry, replacement: string): void;
  addToDictionary(word: string): void;
  openLink(target: LinkRange, text: string): void;
  copyLink(text: string): void;
  runRewrite(actionId: string): void;
  fillPlaceholders(): void;
  searchWorkspace(query: string): void;
  /** Rewrite actions, from the one action table. */
  rewriteActions: ReadonlyArray<{ id: string; menuLabel: string }>;
}

const MOD = IS_MAC ? "⌘" : "Ctrl+";

/** How many spelling suggestions a menu shows before it stops being a menu. */
export const MAX_SUGGESTIONS = 5;

/** Longest selection fragment echoed back in the search row. */
const SEARCH_LABEL_CHARS = 24;

export function buildEditorMenuItems(
  ctx: EditorMenuContext,
  actions: EditorMenuActions,
): EditorMenuItem[] {
  const items: EditorMenuItem[] = [];

  // Spelling first: when the user right-clicks a red-underlined word, the fix
  // is what they came for.
  if (ctx.spelling) {
    const entry = ctx.spelling;
    const suggestions = entry.suggestions.slice(0, MAX_SUGGESTIONS);
    for (const suggestion of suggestions) {
      items.push({
        label: suggestion,
        action: () => actions.applySpelling(entry, suggestion),
        disabled: !ctx.editable,
      });
    }
    if (suggestions.length === 0) {
      items.push({ label: "No suggestions", action: () => {}, disabled: true });
    }
    items.push({
      label: `Add "${entry.word}" to dictionary`,
      action: () => actions.addToDictionary(entry.word),
      separator: suggestions.length > 0,
    });
  }

  if (ctx.link) {
    const link = ctx.link;
    items.push({
      label: "Open link",
      action: () => actions.openLink(link.range, link.text),
      separator: items.length > 0,
    });
    items.push({ label: "Copy link", action: () => actions.copyLink(link.text) });
  }

  const clipboardStart = items.length > 0;
  if (ctx.hasSelection) {
    items.push({
      label: "Cut",
      action: actions.cut,
      disabled: !ctx.editable,
      separator: clipboardStart,
      kbd: `${MOD}X`,
    });
    items.push({ label: "Copy", action: actions.copy, kbd: `${MOD}C` });
    items.push({
      label: "Paste",
      action: actions.paste,
      disabled: !ctx.editable,
      kbd: `${MOD}V`,
    });
  } else {
    items.push({
      label: "Paste",
      action: actions.paste,
      disabled: !ctx.editable,
      separator: clipboardStart,
      kbd: `${MOD}V`,
    });
    items.push({ label: "Select all", action: actions.selectAll, kbd: `${MOD}A` });
  }

  // Rewrite acts on a selection. With nothing selected the whole-document path
  // stays available from the palette and the status-bar chip, so the menu does
  // not offer five rows that all open the same confirmation.
  if (ctx.aiEnabled && ctx.hasSelection) {
    actions.rewriteActions.forEach((action, index) => {
      items.push({
        label: action.menuLabel,
        action: () => actions.runRewrite(action.id),
        disabled: !ctx.editable,
        separator: index === 0,
      });
    });
  }

  if (ctx.hasPlaceholders) {
    items.push({
      label: "Fill placeholders…",
      action: actions.fillPlaceholders,
      separator: true,
    });
  }

  if (ctx.hasSelection) {
    items.push({
      label: `Search workspace for "${truncate(ctx.selectionText, SEARCH_LABEL_CHARS)}"`,
      action: () => actions.searchWorkspace(ctx.selectionText),
      separator: true,
    });
  }

  return items;
}

/** Single-line, ellipsised fragment for a menu label. */
export function truncate(text: string, max: number): string {
  const oneLine = text.replace(/\s+/g, " ").trim();
  return oneLine.length <= max ? oneLine : `${oneLine.slice(0, max)}…`;
}

/** The misspelling covering `pos`, if any. */
export function spellingAt(entries: readonly SpellingEntry[], pos: number): SpellingEntry | null {
  return entries.find((e) => pos >= e.from && pos < e.to) ?? null;
}

/** Whether `pos` falls inside the current selection. */
function posInSelection(view: EditorView, pos: number): boolean {
  return view.state.selection.ranges.some((r) => !r.empty && pos >= r.from && pos <= r.to);
}

export interface EditorContextMenuDeps {
  /** Opens the app menu at viewport coordinates. */
  show(x: number, y: number, items: EditorMenuItem[]): void;
  /** Live spelling entries for the active view. */
  spellingEntries(): readonly SpellingEntry[];
  actions: Omit<EditorMenuActions, "cut" | "copy" | "paste" | "selectAll">;
  /** Clipboard verbs, which need the view. */
  clipboard: {
    cut(view: EditorView): void;
    copy(view: EditorView): void;
    paste(view: EditorView): void;
  };
  aiEnabled(): boolean;
  editable(view: EditorView): boolean;
}

const PLACEHOLDER = /\{\{[A-Za-z_][A-Za-z0-9_]*\}\}/;

export function editorContextMenu(deps: EditorContextMenuDeps): Extension {
  return EditorView.domEventHandlers({
    contextmenu(event, view) {
      const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });

      // WebKit does not move the caret for a right-click, so a click outside an
      // existing selection would otherwise offer that selection's actions while
      // pointing somewhere else. Collapse to the pointer instead, which is what
      // a native editor does.
      if (pos !== null && !posInSelection(view, pos) && !view.state.selection.main.empty) {
        view.dispatch({ selection: { anchor: pos } });
      }

      const selection = view.state.selection.main;
      const selectionText = selection.empty
        ? ""
        : view.state.doc.sliceString(selection.from, selection.to);
      const linkRange =
        pos === null ? null : (findLinkTargets(view.state, pos, pos).find(
          (r) => pos >= r.from && pos < r.to,
        ) ?? null);

      const items = buildEditorMenuItems(
        {
          hasSelection: selectionText.trim().length > 0,
          selectionText,
          spelling: pos === null ? null : spellingAt(deps.spellingEntries(), pos),
          link: linkRange
            ? { range: linkRange, text: view.state.doc.sliceString(linkRange.from, linkRange.to) }
            : null,
          aiEnabled: deps.aiEnabled(),
          hasPlaceholders: PLACEHOLDER.test(view.state.doc.toString()),
          editable: deps.editable(view),
        },
        {
          ...deps.actions,
          cut: () => deps.clipboard.cut(view),
          copy: () => deps.clipboard.copy(view),
          paste: () => deps.clipboard.paste(view),
          selectAll: () =>
            view.dispatch({ selection: { anchor: 0, head: view.state.doc.length } }),
        },
      );

      if (items.length === 0) return false;
      event.preventDefault();
      // A keyboard-invoked menu (menu key / Shift+F10) arrives at 0,0; anchor it
      // to the caret so it does not open in the window corner.
      const coords =
        event.clientX === 0 && event.clientY === 0
          ? view.coordsAtPos(selection.head)
          : { left: event.clientX, top: event.clientY };
      deps.show(coords?.left ?? 0, coords?.top ?? 0, items);
      return true;
    },
  });
}
