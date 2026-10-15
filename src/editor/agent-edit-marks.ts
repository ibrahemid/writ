import { StateEffect, StateField, type EditorState } from "@codemirror/state";
import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";

import { ExternalReloadTxn } from "./reload-annotation";
import "../components/Editor/cm-agent-edit.css";

/** The range a connected program's write put into the document, and who. */
export interface AgentEdit {
  from: number;
  to: number;
  /** The program, by the name a person reads. */
  program: string;
}

/** Marks a range a program wrote. Sent with the reload that brought it in. */
export const markAgentEdit = StateEffect.define<AgentEdit>();

/** Drops every mark, for a tab the person leaves. */
export const clearAgentEdits = StateEffect.define<null>();

function markFor(program: string): Decoration {
  return Decoration.mark({
    class: "cm-agent-edit",
    attributes: { title: `Changed by ${program}` },
  });
}

/**
 * The ranges programs wrote into the document, until the person edits it or
 * leaves the tab.
 *
 * A change the file brought in moves the marks with it, so a second write from
 * a program keeps the first one marked. Any other change to the document is
 * the person's, an undo included, and the marks go with it.
 */
export const agentEditMarks = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(marks, tr) {
    let next = marks;
    if (tr.effects.some((effect) => effect.is(clearAgentEdits))) {
      next = Decoration.none;
    } else if (tr.docChanged) {
      next = tr.annotation(ExternalReloadTxn) === true ? next.map(tr.changes) : Decoration.none;
    }
    for (const effect of tr.effects) {
      if (!effect.is(markAgentEdit)) continue;
      const { from, to, program } = effect.value;
      if (to <= from) continue;
      next = next.update({ add: [markFor(program).range(from, to)] });
    }
    return next;
  },
  provide: (field) => EditorView.decorations.from(field),
});

/** Whether `state` holds any marked range. */
export function hasAgentEdits(state: EditorState): boolean {
  const marks = state.field(agentEditMarks, false);
  return marks !== undefined && marks.size > 0;
}

/** `state` without its marks, for a tab being left. */
export function withoutAgentEdits(state: EditorState): EditorState {
  return hasAgentEdits(state) ? state.update({ effects: clearAgentEdits.of(null) }).state : state;
}

/** The marked ranges in `state`, in document order. */
export function agentEditRanges(state: EditorState): { from: number; to: number; title: string }[] {
  const ranges: { from: number; to: number; title: string }[] = [];
  const marks = state.field(agentEditMarks, false);
  if (!marks) return ranges;
  const cursor = marks.iter();
  while (cursor.value) {
    const title = (cursor.value.spec.attributes as { title?: string } | undefined)?.title ?? "";
    ranges.push({ from: cursor.from, to: cursor.to, title });
    cursor.next();
  }
  return ranges;
}
