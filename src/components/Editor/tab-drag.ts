import { createSignal } from "solid-js";
import type { PaneId } from "../../lib/pane-layout";

// Singleton state — Writ is single-window. The tab strips a dragged tab can
// land on, by pane, and the drag under way. Each strip is a TabBar of its
// own; this is what lets one see a tab dragged out of the other (ADR-056).

const strips = new Map<PaneId, HTMLElement>();
const [dragged, setDragged] = createSignal<{ id: string; from: PaneId } | null>(null);
const [over, setOver] = createSignal<PaneId | null>(null);

/** The tab being dragged and the pane it left, or null. */
export const draggedTab = dragged;

/** The pane whose strip the dragged tab is over, or null. */
export const dropPane = over;

/** Makes `el` the strip of `pane`; answers the call that stops it. */
export function registerStrip(pane: PaneId, el: HTMLElement): () => void {
  strips.set(pane, el);
  return () => {
    if (strips.get(pane) === el) strips.delete(pane);
  };
}

function stripAt(x: number, y: number): PaneId | null {
  for (const [pane, el] of strips) {
    const box = el.getBoundingClientRect();
    if (x >= box.left && x <= box.right && y >= box.top && y <= box.bottom) return pane;
  }
  return null;
}

export function startTabDrag(id: string, from: PaneId): void {
  setDragged({ id, from });
  setOver(null);
}

export function moveTabDrag(x: number, y: number): void {
  setOver(stripAt(x, y));
}

/** Ends the drag; answers the pane whose strip it ended on, or null. */
export function endTabDrag(x: number | null, y: number | null): PaneId | null {
  const target = x === null || y === null ? null : stripAt(x, y);
  setDragged(null);
  setOver(null);
  return target;
}
