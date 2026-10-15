import { createSignal } from "solid-js";
import {
  closeSplit as closeSplitLayout,
  focus as focusLayout,
  fromStored,
  holds,
  isSplit,
  moveToOther as moveToOtherLayout,
  panesOf,
  removeFromPane as removeFromPaneLayout,
  sameLayout,
  select as selectLayout,
  selectIn as selectInLayout,
  settle as settleLayout,
  SINGLE_PANE,
  splitToOther as splitToOtherLayout,
  tabsIn,
  toStored,
  withRatio,
  type PaneId,
  type PaneLayout,
} from "../../lib/pane-layout";
import { logFailure } from "../../lib/log";

export type PaneStore = ReturnType<typeof createPaneStore>;

/** Where the layout is kept between launches: the webview's own storage (ADR-056). */
export const PANES_STORAGE_KEY = "writ.editor.panes.v1";

/**
 * Hears a tab leaving a pane: `to` is the pane it moved to, or null when it is
 * only gone from `from`. Called before the layout changes, while the pane it
 * leaves still holds its editor.
 */
export type TabLeavesPane = (from: PaneId, to: PaneId | null, id: string) => void;

// The panes of one window (ADR-056). The layout is a value; every change goes
// through `commit`, which tells the editor which tabs leave which pane before
// the panes move, and keeps the layout once the launch's own has been read.

export function createPaneStore() {
  const [layout, setLayout] = createSignal<PaneLayout>(SINGLE_PANE, { equals: sameLayout });
  const leaveListeners = new Set<TabLeavesPane>();
  let restored = false;

  function persist(next: PaneLayout): void {
    try {
      localStorage.setItem(PANES_STORAGE_KEY, JSON.stringify(toStored(next)));
    } catch {
      // Storage refused (private mode, quota): the panes still work, and the
      // next launch opens with one.
    }
  }

  // The tabs in `open` whose panes change, told to the listeners first. A tab
  // that is closing is not in `open`, so nothing is handed over for it.
  function commit(next: PaneLayout, open: readonly string[] | null): void {
    const prev = layout();
    if (sameLayout(prev, next)) return;
    const openSet = open === null ? null : new Set(open);
    const changed = new Set([...prev.placement.keys(), ...next.placement.keys()]);
    for (const id of changed) {
      if (openSet !== null && !openSet.has(id)) continue;
      const before = panesOf(prev, id);
      const after = panesOf(next, id);
      const gained = after.find((pane) => !before.includes(pane)) ?? null;
      for (const pane of before) {
        if (after.includes(pane)) continue;
        for (const listener of leaveListeners) {
          try {
            listener(pane, gained, id);
          } catch {
            logFailure("an editor could not be handed to the other pane");
          }
        }
      }
    }
    setLayout(next);
    if (restored) persist(next);
  }

  /** Hears every tab that leaves a pane; answers the call that stops it. */
  function onTabLeavesPane(listener: TabLeavesPane): () => void {
    leaveListeners.add(listener);
    return () => {
      leaveListeners.delete(listener);
    };
  }

  /**
   * Reads the layout the last launch left, against the tabs open now, and
   * keeps every later change.
   */
  function restore(open: readonly string[]): void {
    let raw: unknown = null;
    try {
      const text = localStorage.getItem(PANES_STORAGE_KEY);
      raw = text ? JSON.parse(text) : null;
    } catch {
      raw = null;
    }
    setLayout(fromStored(raw, open));
    restored = true;
    persist(layout());
  }

  return {
    layout,
    focused: (): PaneId => layout().focused,
    isSplit: (): boolean => isSplit(layout()),
    ratio: (): number => layout().ratio,
    activeIn: (pane: PaneId): string | null => layout().active[pane],
    holds: (pane: PaneId, id: string): boolean => holds(layout(), pane, id),
    panesOf: (id: string): PaneId[] => panesOf(layout(), id),
    tabsIn: (pane: PaneId, open: readonly string[]): string[] => tabsIn(layout(), pane, open),
    onTabLeavesPane,
    restore,

    focus(pane: PaneId): void {
      commit(focusLayout(layout(), pane), null);
    },

    /** Selects `id` where it is shown; a tab just opened joins the focused pane. */
    select(id: string | null, fresh: boolean): void {
      commit(selectLayout(layout(), id, fresh), null);
    },

    selectIn(pane: PaneId, id: string): void {
      commit(selectInLayout(layout(), pane, id), null);
    },

    splitToOther(): void {
      commit(splitToOtherLayout(layout()), null);
    },

    /** Takes `id` out of `pane`; answers whether the other pane still shows it. */
    removeFromPane(pane: PaneId, id: string, open: readonly string[]): boolean {
      const { layout: next, stillShown } = removeFromPaneLayout(layout(), pane, id, open);
      commit(next, stillShown ? open : open.filter((other) => other !== id));
      return stillShown;
    },

    moveToOther(from: PaneId, id: string, open: readonly string[]): void {
      commit(moveToOtherLayout(layout(), from, id, open), open);
    },

    closeSplit(): void {
      commit(closeSplitLayout(layout()), null);
    },

    /** The divider's place; `keep` writes it, once a drag ends. */
    setRatio(ratio: number, keep: boolean): void {
      const next = withRatio(layout(), ratio);
      setLayout(next);
      if (keep && restored) persist(next);
    },

    /** Drops what closed tabs left behind and collapses an empty pane. */
    settle(open: readonly string[]): void {
      commit(settleLayout(layout(), open), open);
    },
  };
}
