import { describe, it, expect } from "vitest";
import {
  closeSplit,
  fromStored,
  holds,
  isSplit,
  moveToOther,
  removeFromPane,
  select,
  selectIn,
  settle,
  SINGLE_PANE,
  splitToOther,
  tabsIn,
  toStored,
  withRatio,
  type PaneLayout,
} from "../../lib/pane-layout";

// The pane rules of ADR-056 as data: which pane shows a tab, which tab is in
// front of each, which pane has focus, and when a pane collapses.

const OPEN = ["a", "b", "c"];

function shownIn(layout: PaneLayout, open: readonly string[] = OPEN) {
  return { left: tabsIn(layout, "left", open), right: tabsIn(layout, "right", open) };
}

/** a and b on the left with b in front, c on the right in front, focus right. */
function splitWithC(): PaneLayout {
  const onLeft = select(SINGLE_PANE, "a", false);
  return moveToOther(select(onLeft, "c", false), "left", "c", OPEN);
}

describe("one pane", () => {
  it("holds every open tab on the left, with nothing split", () => {
    const layout = select(SINGLE_PANE, "b", false);
    expect(shownIn(layout)).toEqual({ left: OPEN, right: [] });
    expect(layout.active).toEqual({ left: "b", right: null });
    expect(isSplit(layout)).toBe(false);
  });
});

describe("Split right", () => {
  it("shows the tab in front in both panes and focuses the new one", () => {
    const layout = splitToOther(select(SINGLE_PANE, "a", false));
    expect(shownIn(layout)).toEqual({ left: OPEN, right: ["a"] });
    expect(layout.active).toEqual({ left: "a", right: "a" });
    expect(layout.focused).toBe("right");
    expect(isSplit(layout)).toBe(true);
  });

  it("does nothing with no tab in front", () => {
    expect(splitToOther(SINGLE_PANE)).toBe(SINGLE_PANE);
  });
});

describe("selecting a tab", () => {
  it("selects an open tab in the pane that shows it and moves focus there", () => {
    const layout = select(splitWithC(), "a", false);
    expect(layout.focused).toBe("left");
    expect(layout.active).toEqual({ left: "a", right: "c" });
  });

  it("puts a tab that was just opened in the focused pane", () => {
    const layout = select(splitWithC(), "d", true);
    expect(holds(layout, "right", "d")).toBe(true);
    expect(holds(layout, "left", "d")).toBe(false);
    expect(layout.active.right).toBe("d");
  });

  it("selects in a named pane only when that pane shows the tab", () => {
    const layout = splitWithC();
    expect(selectIn(layout, "right", "a")).toBe(layout);
    expect(selectIn(layout, "left", "b").active.left).toBe("b");
    expect(selectIn(layout, "left", "b").focused).toBe("left");
  });
});

describe("taking a tab out of a pane", () => {
  it("keeps a tab the other pane shows open there", () => {
    const both = splitToOther(select(SINGLE_PANE, "a", false));
    const { layout, stillShown } = removeFromPane(both, "right", "a", OPEN);
    expect(stillShown).toBe(true);
    expect(isSplit(layout)).toBe(false);
    expect(layout.focused).toBe("left");
    expect(layout.active.left).toBe("a");
  });

  it("collapses the right pane on its last tab, focus back on the left", () => {
    const { layout, stillShown } = removeFromPane(splitWithC(), "right", "c", OPEN);
    expect(stillShown).toBe(false);
    expect(isSplit(layout)).toBe(false);
    expect(layout.focused).toBe("left");
    expect(layout.active).toEqual({ left: "b", right: null });
  });

  it("moves the right pane's tabs left when the left pane loses its last tab", () => {
    const onlyA = ["a", "c"];
    const split = moveToOther(select(SINGLE_PANE, "c", false), "left", "c", onlyA);
    const { layout } = removeFromPane(select(split, "a", false), "left", "a", onlyA);
    expect(shownIn(layout, ["c"])).toEqual({ left: ["c"], right: [] });
    expect(layout.active).toEqual({ left: "c", right: null });
    expect(layout.focused).toBe("left");
  });

  it("selects the pane's last other tab when the one in front goes", () => {
    const { layout } = removeFromPane(select(SINGLE_PANE, "c", false), "left", "c", OPEN);
    expect(layout.active.left).toBe("b");
  });
});

describe("moving a tab to the other pane", () => {
  it("shows it there in front, with focus, and takes it out of the pane it left", () => {
    const layout = splitWithC();
    expect(shownIn(layout)).toEqual({ left: ["a", "b"], right: ["c"] });
    expect(layout.focused).toBe("right");
    expect(layout.active).toEqual({ left: "b", right: "c" });
  });

  it("only leaves a pane when the other pane already shows the tab", () => {
    const both = splitToOther(select(SINGLE_PANE, "a", false));
    const layout = moveToOther(both, "left", "a", OPEN);
    expect(shownIn(layout)).toEqual({ left: ["b", "c"], right: ["a"] });
    expect(layout.active).toEqual({ left: "c", right: "a" });
  });
});

describe("Close split", () => {
  it("puts every tab in one pane with the focused pane's tab in front", () => {
    const layout = closeSplit(splitWithC());
    expect(shownIn(layout)).toEqual({ left: OPEN, right: [] });
    expect(layout.active).toEqual({ left: "c", right: null });
    expect(layout.focused).toBe("left");
  });
});

describe("settling against the open tabs", () => {
  it("drops closed tabs and collapses a pane they leave empty", () => {
    const layout = settle(splitWithC(), ["a", "b"]);
    expect(isSplit(layout)).toBe(false);
    expect(layout.active).toEqual({ left: "b", right: null });
  });

  it("keeps a pane with no tab in front that way: a download is showing", () => {
    const layout = settle(select(select(SINGLE_PANE, "a", false), null, false), OPEN);
    expect(layout.active.left).toBeNull();
  });
});

describe("the layout kept between launches", () => {
  it("comes back as it was left", () => {
    const left = withRatio(splitWithC(), 0.3);
    expect(fromStored(JSON.parse(JSON.stringify(toStored(left))), OPEN)).toEqual(left);
  });

  it("drops ids no open tab answers to and collapses a pane left empty", () => {
    const kept = toStored(splitWithC());
    const layout = fromStored(kept, ["a", "b"]);
    expect(isSplit(layout)).toBe(false);
    expect(layout.focused).toBe("left");
    expect(layout.active.left).toBe("b");
  });

  it("reads anything else as one pane on the last tab", () => {
    for (const raw of [null, "x", { v: 2 }, { v: 1, right: "c", ratio: "wide" }]) {
      const layout = fromStored(raw, OPEN);
      expect(isSplit(layout)).toBe(false);
      expect(layout.active.left).toBe("c");
    }
  });

  it("keeps the divider within its range", () => {
    expect(withRatio(SINGLE_PANE, 0.95).ratio).toBe(0.8);
    expect(withRatio(SINGLE_PANE, Number.NaN).ratio).toBe(0.5);
  });
});
