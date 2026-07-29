import { EditorView } from "@codemirror/view";
import type { Extension } from "@codemirror/state";
import type { SpellingEntry } from "./spelling";
import { spellingAt, MAX_SUGGESTIONS, type EditorMenuItem } from "./context-menu";

/**
 * Double-click a misspelled word to see its corrections above it; click one to
 * apply it.
 *
 * Before this, a flagged word could only be fixed in bulk ("Fix all") or
 * silenced ("Add to dictionary") — there was no way to accept a single
 * correction, and no way to see the alternatives at the word itself.
 *
 * Bound to double-click rather than a single click on purpose: clicking into a
 * word to place the caret is how text gets edited, and a popover on every such
 * click would fight the user. Double-click already selects the word, so nothing
 * is taken away. The right-click menu offers the same corrections.
 */

export interface SpellingMenuDeps {
  /** Opens the menu against `rect`, confined to `bounds`. */
  showAt(rect: DOMRect, items: EditorMenuItem[], bounds: DOMRect): void;
  entries(): readonly SpellingEntry[];
  apply(entry: SpellingEntry, replacement: string): void;
  addToDictionary(word: string): void;
}

/** Rows for one flagged word: its corrections, then the per-word dictionary
 * action. Pure, so the ordering is testable without an editor. */
export function spellingMenuItems(
  entry: SpellingEntry,
  deps: Pick<SpellingMenuDeps, "apply" | "addToDictionary">,
): EditorMenuItem[] {
  const suggestions = entry.suggestions.slice(0, MAX_SUGGESTIONS);
  const items: EditorMenuItem[] = suggestions.map((suggestion) => ({
    label: suggestion,
    action: () => deps.apply(entry, suggestion),
  }));
  if (items.length === 0) {
    items.push({ label: "No suggestions", action: () => {}, disabled: true });
  }
  items.push({
    label: `Add "${entry.word}" to dictionary`,
    action: () => deps.addToDictionary(entry.word),
    separator: items.length > 0,
  });
  return items;
}

/** Screen rect covering `[from, to)`, used to anchor the menu over the word. */
export function wordRect(view: EditorView, from: number, to: number): DOMRect | null {
  const start = view.coordsAtPos(from);
  const end = view.coordsAtPos(to) ?? start;
  if (!start || !end) return null;
  const left = Math.min(start.left, end.left);
  const right = Math.max(start.right, end.right);
  const top = Math.min(start.top, end.top);
  const bottom = Math.max(start.bottom, end.bottom);
  return new DOMRect(left, top, right - left, bottom - top);
}

export function spellingMenu(deps: SpellingMenuDeps): Extension {
  return EditorView.domEventHandlers({
    dblclick(event, view) {
      if (event.button !== 0) return false;
      const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
      if (pos === null) return false;
      const entry = spellingAt(deps.entries(), pos);
      if (!entry) return false;

      const rect = wordRect(view, entry.from, entry.to);
      if (!rect) return false;

      event.preventDefault();
      // Confined to the editor's scroller, so corrections for a word on the
      // first line open below it rather than over the tab bar.
      deps.showAt(rect, spellingMenuItems(entry, deps), view.scrollDOM.getBoundingClientRect());
      return true;
    },
  });
}
