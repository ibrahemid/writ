import { isolateHistory } from "@codemirror/commands";
import { EditorSelection, StateEffect, StateField, type EditorState } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet } from "@codemirror/view";
import { joinDictation } from "../lib/dictation-text";

/**
 * Where a dictation session writes, mapped through every change made while it
 * runs. Before the first segment it covers the selection the session started
 * on, which that segment replaces; after it, the point at the end of the last
 * inserted text. A replaced editor state drops it, and the session's next
 * segment then reports `anchor-lost`.
 */
interface DictationAnchor {
  from: number;
  to: number;
  /** Text the engine is still revising, spaced as it would be inserted, drawn
   * after `to` and not in the document. */
  partial: string;
}

const setAnchor = StateEffect.define<{ from: number; to: number } | null>();
const setPartial = StateEffect.define<string>();

const anchorField = StateField.define<DictationAnchor | null>({
  create: () => null,
  update(anchor, tr) {
    let next = anchor;
    if (next && tr.docChanged) {
      if (next.from === next.to) {
        const at = tr.changes.mapPos(next.from, 1);
        next = { ...next, from: at, to: at };
      } else {
        const from = tr.changes.mapPos(next.from, 1);
        next = { ...next, from, to: Math.max(from, tr.changes.mapPos(next.to, -1)) };
      }
    }
    for (const effect of tr.effects) {
      if (effect.is(setAnchor)) {
        next = effect.value ? { ...effect.value, partial: "" } : null;
      } else if (effect.is(setPartial) && next) {
        const before = tr.state.doc.sliceString(Math.max(0, next.to - 2), next.to);
        next = { ...next, partial: joinDictation(before, effect.value, "") };
      }
    }
    return next;
  },
  provide: (field) =>
    EditorView.decorations.from(field, (anchor): DecorationSet => {
      if (!anchor || anchor.partial === "") return Decoration.none;
      return Decoration.set([
        Decoration.widget({ widget: new PartialWidget(anchor.partial), side: 1 }).range(anchor.to),
      ]);
    }),
});

class PartialWidget extends WidgetType {
  constructor(readonly text: string) {
    super();
  }

  override eq(other: PartialWidget): boolean {
    return other.text === this.text;
  }

  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className = "cm-dictation-partial";
    span.dir = "auto";
    span.textContent = this.text;
    return span;
  }

  override ignoreEvent(): boolean {
    return true;
  }
}

const partialTheme = EditorView.baseTheme({
  ".cm-dictation-partial": { color: "var(--jotra-fg-muted)" },
});

function anchorOf(view: EditorView): DictationAnchor | null {
  return view.state.field(anchorField, false) ?? null;
}

/**
 * Anchors a session at the main selection. Answers false, and anchors
 * nothing, when the document is read-only.
 */
export function startDictationAnchor(view: EditorView): boolean {
  if (view.state.readOnly) return false;
  // Installed on first use, in a transaction of its own: a field added by a
  // reconfiguration starts from `create`, so an effect sent with it is lost.
  if (view.state.field(anchorField, false) === undefined) {
    view.dispatch({ effects: StateEffect.appendConfig.of([anchorField, partialTheme]) });
  }
  const { from, to } = view.state.selection.main;
  view.dispatch({ effects: setAnchor.of({ from, to }) });
  return true;
}

/** Shows the engine's revisable text at the anchor, outside the document. */
export function showDictationPartial(view: EditorView, text: string): void {
  if (!anchorOf(view)) return;
  view.dispatch({ effects: setPartial.of(text) });
}

/**
 * Inserts one finished segment at the anchor as its own undo step. The first
 * segment replaces the selection the session started on. The caret follows
 * only when it sat at the anchor.
 */
export function insertDictationSegment(view: EditorView, text: string): "inserted" | "anchor-lost" {
  const anchor = anchorOf(view);
  if (!anchor || view.state.readOnly) return "anchor-lost";
  const { doc, selection } = view.state;
  const before = doc.sliceString(Math.max(0, anchor.from - 2), anchor.from);
  const after = doc.sliceString(anchor.to, Math.min(doc.length, anchor.to + 2));
  const insert = joinDictation(before, text, after);
  if (insert === "") {
    view.dispatch({ effects: setPartial.of("") });
    return "inserted";
  }
  const end = anchor.from + insert.trimEnd().length;
  const main = selection.main;
  const caretAtAnchor =
    (main.from === anchor.from && main.to === anchor.to) || (main.empty && main.head === anchor.to);
  view.dispatch({
    changes: { from: anchor.from, to: anchor.to, insert },
    effects: [setAnchor.of({ from: end, to: end })],
    annotations: isolateHistory.of("full"),
    userEvent: "input.dictation",
    ...(caretAtAnchor ? { selection: EditorSelection.cursor(end), scrollIntoView: true } : {}),
  });
  return "inserted";
}

/** Clears the partial text and the anchor. */
export function endDictationAnchor(view: EditorView): void {
  if (!anchorOf(view)) return;
  view.dispatch({ effects: setAnchor.of(null) });
}

/**
 * `state` with no anchor or partial text, for a tab's state kept while
 * another tab shows: the session ends on the view after the swap, and would
 * otherwise come back drawn when the tab does.
 */
export function withoutDictationAnchor(state: EditorState): EditorState {
  return state.field(anchorField, false) ? state.update({ effects: setAnchor.of(null) }).state : state;
}
