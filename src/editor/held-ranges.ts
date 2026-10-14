import { StateEffect, StateField } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";

/**
 * Ranges an edit holds while its replacement text is computed elsewhere (a
 * transform over IPC), mapped through every change made in the meantime.
 *
 * An edit that lands on its original offsets after the person kept typing
 * above them overwrites what they typed. A held range follows the text it
 * covers instead, and a change that touches the range itself drops it: the
 * replacement was computed from text that is no longer there.
 */

interface HeldRange {
  from: number;
  to: number;
  /** False once a change touched the range; the edit must not land. */
  intact: boolean;
}

const hold = StateEffect.define<{ id: number; from: number; to: number }>();
const release = StateEffect.define<number>();

const heldRanges = StateField.define<ReadonlyMap<number, HeldRange>>({
  create: () => new Map(),
  update(ranges, tr) {
    let next = ranges;
    if (tr.docChanged && ranges.size > 0) {
      const mapped = new Map<number, HeldRange>();
      for (const [id, range] of ranges) {
        if (!range.intact || tr.changes.touchesRange(range.from, range.to)) {
          mapped.set(id, { ...range, intact: false });
        } else {
          mapped.set(id, {
            from: tr.changes.mapPos(range.from, 1),
            to: tr.changes.mapPos(range.to, -1),
            intact: true,
          });
        }
      }
      next = mapped;
    }
    for (const effect of tr.effects) {
      if (effect.is(hold)) {
        const added = new Map(next);
        added.set(effect.value.id, { from: effect.value.from, to: effect.value.to, intact: true });
        next = added;
      } else if (effect.is(release) && next.has(effect.value)) {
        const removed = new Map(next);
        removed.delete(effect.value);
        next = removed;
      }
    }
    return next;
  },
});

let nextId = 0;

/** Starts following `from`..`to` in `view`; the answer names the hold. */
export function holdRange(view: EditorView, from: number, to: number): number {
  // Installed on first use, in a transaction of its own: a field added by a
  // reconfiguration starts from `create`, so an effect sent with it is lost.
  if (view.state.field(heldRanges, false) === undefined) {
    view.dispatch({ effects: StateEffect.appendConfig.of(heldRanges) });
  }
  nextId += 1;
  view.dispatch({ effects: hold.of({ id: nextId, from, to }) });
  return nextId;
}

/**
 * Ends hold `id` and answers where its range is now, or null when a change
 * touched it (or the view's state was replaced, which drops every hold).
 */
export function releaseRange(view: EditorView, id: number): { from: number; to: number } | null {
  const range = view.state.field(heldRanges, false)?.get(id);
  if (range === undefined) return null;
  view.dispatch({ effects: release.of(id) });
  return range.intact ? { from: range.from, to: range.to } : null;
}
