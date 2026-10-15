// The two editor panes as data: which pane shows each open tab, the tab in
// front in each, which pane has focus and how the width is shared. Every rule
// is a pure function of the layout and the open tabs in tab order, so the
// stores and the tests read the same answers (ADR-056).

export type PaneId = "left" | "right";

export const PANE_IDS: readonly PaneId[] = ["left", "right"];

/** Where an open tab is shown. A tab with no entry is in the left pane only. */
export type Placement = "right" | "both";

export interface PaneLayout {
  readonly placement: ReadonlyMap<string, Placement>;
  readonly active: Readonly<Record<PaneId, string | null>>;
  readonly focused: PaneId;
  /** The left pane's share of the width while both show. */
  readonly ratio: number;
}

export const MIN_PANE_RATIO = 0.2;
export const MAX_PANE_RATIO = 0.8;
export const DEFAULT_PANE_RATIO = 0.5;

export const SINGLE_PANE: PaneLayout = {
  placement: new Map(),
  active: { left: null, right: null },
  focused: "left",
  ratio: DEFAULT_PANE_RATIO,
};

export function otherPane(pane: PaneId): PaneId {
  return pane === "left" ? "right" : "left";
}

export function clampPaneRatio(ratio: number): number {
  if (!Number.isFinite(ratio)) return DEFAULT_PANE_RATIO;
  return Math.min(MAX_PANE_RATIO, Math.max(MIN_PANE_RATIO, ratio));
}

/** Whether `pane` shows tab `id`. Needs no list of open tabs. */
export function holds(layout: PaneLayout, pane: PaneId, id: string): boolean {
  const placed = layout.placement.get(id);
  if (placed === "both") return true;
  return pane === "right" ? placed === "right" : placed === undefined;
}

/** The panes that show tab `id`. */
export function panesOf(layout: PaneLayout, id: string): PaneId[] {
  return PANE_IDS.filter((pane) => holds(layout, pane, id));
}

/** The tabs `pane` shows, in tab order. */
export function tabsIn(layout: PaneLayout, pane: PaneId, open: readonly string[]): string[] {
  return open.filter((id) => holds(layout, pane, id));
}

/** Whether the right pane shows anything. Settled layouts name open tabs only. */
export function isSplit(layout: PaneLayout): boolean {
  return layout.placement.size > 0;
}

function withPlacement(layout: PaneLayout, id: string, placed: Placement | null): Map<string, Placement> {
  const next = new Map(layout.placement);
  if (placed === null) next.delete(id);
  else next.set(id, placed);
  return next;
}

/** The placement that shows a tab in exactly `panes`; null for the left alone. */
function placementFor(panes: ReadonlySet<PaneId>): Placement | null {
  if (panes.has("left") && panes.has("right")) return "both";
  return panes.has("right") ? "right" : null;
}

function withActive(layout: PaneLayout, pane: PaneId, id: string | null): PaneLayout {
  return { ...layout, active: { ...layout.active, [pane]: id } };
}

/** The last of `pane`'s tabs in `open`, leaving `except` out. */
function lastTab(layout: PaneLayout, pane: PaneId, open: readonly string[], except?: string): string | null {
  const tabs = tabsIn(layout, pane, open).filter((id) => id !== except);
  return tabs.length > 0 ? tabs[tabs.length - 1] : null;
}

/**
 * Selects tab `id`. A tab already open is selected where it is shown: in the
 * focused pane when that shows it, else in the other pane, which takes focus.
 * A tab just opened (`fresh`) joins the focused pane. Null clears the focused
 * pane's selection.
 */
export function select(layout: PaneLayout, id: string | null, fresh: boolean): PaneLayout {
  const { focused } = layout;
  if (id === null) return withActive(layout, focused, null);
  if (!fresh || layout.placement.has(id)) {
    if (holds(layout, focused, id)) return withActive(layout, focused, id);
    const other = otherPane(focused);
    if (holds(layout, other, id)) return { ...withActive(layout, other, id), focused: other };
  }
  const placed = focused === "right" ? "right" : null;
  return withActive({ ...layout, placement: withPlacement(layout, id, placed) }, focused, id);
}

/** Selects `id` in `pane`, which takes focus. A pane that does not show it is left alone. */
export function selectIn(layout: PaneLayout, pane: PaneId, id: string): PaneLayout {
  if (!holds(layout, pane, id)) return layout;
  return { ...withActive(layout, pane, id), focused: pane };
}

export function focus(layout: PaneLayout, pane: PaneId): PaneLayout {
  return layout.focused === pane ? layout : { ...layout, focused: pane };
}

/**
 * Drops what no open tab needs and collapses a pane left with no tabs.
 *
 * A pane whose tab in front closed shows its last tab; one with no tab in
 * front keeps none, since a download's pane is showing. The left pane holds
 * tabs whenever any are open: when it has none during a split, the right
 * pane's tabs move into it with the tab in front, and focus is the left's.
 */
export function settle(layout: PaneLayout, open: readonly string[]): PaneLayout {
  const openSet = new Set(open);
  let next: PaneLayout = {
    ...layout,
    placement: new Map([...layout.placement].filter(([id]) => openSet.has(id))),
  };
  for (const pane of PANE_IDS) {
    const current = next.active[pane];
    if (current !== null && !(openSet.has(current) && holds(next, pane, current))) {
      next = withActive(next, pane, lastTab(next, pane, open));
    }
  }
  const right = tabsIn(next, "right", open);
  if (right.length === 0) {
    return { ...next, placement: new Map(), focused: "left", active: { left: next.active.left, right: null } };
  }
  if (tabsIn(next, "left", open).length === 0) {
    const front = next.active.right ?? right[right.length - 1];
    return { ...next, placement: new Map(), focused: "left", active: { left: front, right: null } };
  }
  if (next.active.right === null) next = withActive(next, "right", right[right.length - 1]);
  return next;
}

/**
 * Shows the focused pane's tab in the other pane as well, which takes focus.
 * With no tab in front there is nothing to show twice.
 */
export function splitToOther(layout: PaneLayout): PaneLayout {
  const id = layout.active[layout.focused];
  if (id === null) return layout;
  const other = otherPane(layout.focused);
  return { ...withActive({ ...layout, placement: withPlacement(layout, id, "both") }, other, id), focused: other };
}

/**
 * Takes tab `id` out of `pane`, selecting the pane's last other tab when it
 * was in front, and settles against `open`. `stillShown` says whether the
 * other pane shows the tab: when it does not, the caller closes the tab, and
 * the layout is already settled as if it had.
 */
export function removeFromPane(
  layout: PaneLayout,
  pane: PaneId,
  id: string,
  open: readonly string[],
): { layout: PaneLayout; stillShown: boolean } {
  const panes = new Set(panesOf(layout, id));
  if (!panes.has(pane)) return { layout, stillShown: panes.size > 0 };
  panes.delete(pane);
  const stillShown = panes.size > 0;
  const remaining = stillShown ? open : open.filter((other) => other !== id);
  let next: PaneLayout = { ...layout, placement: withPlacement(layout, id, stillShown ? placementFor(panes) : null) };
  if (layout.active[pane] === id) next = withActive(next, pane, lastTab(next, pane, remaining, id));
  return { layout: settle(next, remaining), stillShown };
}

/**
 * Moves tab `id` from `from` to the other pane, which shows it in front and
 * takes focus. A tab the other pane already shows is only taken out of `from`.
 */
export function moveToOther(layout: PaneLayout, from: PaneId, id: string, open: readonly string[]): PaneLayout {
  if (!holds(layout, from, id)) return layout;
  const to = otherPane(from);
  const panes = new Set(panesOf(layout, id));
  panes.delete(from);
  panes.add(to);
  let next: PaneLayout = {
    ...withActive({ ...layout, placement: withPlacement(layout, id, placementFor(panes)) }, to, id),
    focused: to,
  };
  if (layout.active[from] === id) next = withActive(next, from, lastTab(next, from, open, id));
  return settle(next, open);
}

/** One pane again: every tab in the left one, the focused pane's tab in front. */
export function closeSplit(layout: PaneLayout): PaneLayout {
  const front = layout.active[layout.focused] ?? layout.active.left ?? layout.active.right;
  return { ...layout, placement: new Map(), focused: "left", active: { left: front, right: null } };
}

export function withRatio(layout: PaneLayout, ratio: number): PaneLayout {
  const clamped = clampPaneRatio(ratio);
  return clamped === layout.ratio ? layout : { ...layout, ratio: clamped };
}

export function sameLayout(a: PaneLayout, b: PaneLayout): boolean {
  if (a === b) return true;
  if (a.focused !== b.focused || a.ratio !== b.ratio) return false;
  if (a.active.left !== b.active.left || a.active.right !== b.active.right) return false;
  if (a.placement.size !== b.placement.size) return false;
  for (const [id, placed] of a.placement) if (b.placement.get(id) !== placed) return false;
  return true;
}

/** The layout as it is kept between launches. */
export interface StoredPanes {
  v: 1;
  ratio: number;
  focused: PaneId;
  right: string[];
  both: string[];
  active: { left: string | null; right: string | null };
}

export function toStored(layout: PaneLayout): StoredPanes {
  const right: string[] = [];
  const both: string[] = [];
  for (const [id, placed] of layout.placement) (placed === "both" ? both : right).push(id);
  return { v: 1, ratio: layout.ratio, focused: layout.focused, right, both, active: { ...layout.active } };
}

function readStored(raw: unknown): PaneLayout {
  const kept = (typeof raw === "object" && raw !== null ? raw : {}) as Record<string, unknown>;
  if (kept.v !== 1) return SINGLE_PANE;
  const placement = new Map<string, Placement>();
  for (const id of ids(kept.right)) placement.set(id, "right");
  for (const id of ids(kept.both)) placement.set(id, "both");
  const active = (typeof kept.active === "object" && kept.active !== null ? kept.active : {}) as Record<string, unknown>;
  return {
    placement,
    active: { left: idOrNull(active.left), right: idOrNull(active.right) },
    focused: kept.focused === "right" ? "right" : "left",
    ratio: clampPaneRatio(typeof kept.ratio === "number" ? kept.ratio : DEFAULT_PANE_RATIO),
  };
}

function idOrNull(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function ids(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((id): id is string => typeof id === "string") : [];
}

/**
 * Reads a kept layout against the tabs open now. Anything that does not read
 * is left out, an id no open tab answers to is dropped, a pane left with no
 * tabs collapses, and a pane with tabs shows one in front.
 */
export function fromStored(raw: unknown, open: readonly string[]): PaneLayout {
  const restored = settle(readStored(raw), open);
  const front = restored.active.left ?? lastTab(restored, "left", open);
  const withFront = withActive(restored, "left", front);
  return isSplit(withFront) ? withFront : { ...withFront, focused: "left" };
}
