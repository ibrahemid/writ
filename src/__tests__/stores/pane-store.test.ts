import { describe, it, expect, beforeEach } from "vitest";
import { createRoot } from "solid-js";
import { createPaneStore, PANES_STORAGE_KEY } from "../../stores/window/pane-store";

// The window's panes (ADR-056): the editor hears which tabs leave which pane
// before the panes move, and the layout comes back after a relaunch.

const OPEN = ["a", "b", "c"];

function freshStore() {
  return createRoot(() => createPaneStore());
}

beforeEach(() => {
  localStorage.clear();
});

describe("a tab leaving a pane", () => {
  it("is told to the editor with the pane it moved to, while the old layout stands", () => {
    const panes = freshStore();
    panes.select("c", false);
    const heard: Array<[string, string | null, string, string | null]> = [];
    panes.onTabLeavesPane((from, to, id) => heard.push([from, to, id, panes.activeIn("right")]));
    panes.moveToOther("left", "c", OPEN);
    expect(heard).toEqual([["left", "right", "c", null]]);
    expect(panes.activeIn("right")).toBe("c");
  });

  it("is told with no pane when it only left one of the two showing it", () => {
    const panes = freshStore();
    panes.select("a", false);
    panes.splitToOther();
    const heard: Array<[string, string | null, string]> = [];
    panes.onTabLeavesPane((from, to, id) => heard.push([from, to, id]));
    expect(panes.removeFromPane("right", "a", OPEN)).toBe(true);
    expect(heard).toEqual([["right", null, "a"]]);
  });

  it("is not told for a tab that is closing", () => {
    const panes = freshStore();
    panes.select("c", false);
    panes.moveToOther("left", "c", OPEN);
    const heard: string[] = [];
    panes.onTabLeavesPane((_from, _to, id) => heard.push(id));
    expect(panes.removeFromPane("right", "c", OPEN)).toBe(false);
    expect(heard).toEqual([]);
    expect(panes.isSplit()).toBe(false);
  });

  it("hands the right pane's tabs to the left when the split closes", () => {
    const panes = freshStore();
    panes.select("c", false);
    panes.moveToOther("left", "c", OPEN);
    const heard: Array<[string, string | null, string]> = [];
    panes.onTabLeavesPane((from, to, id) => heard.push([from, to, id]));
    panes.closeSplit();
    expect(heard).toEqual([["right", "left", "c"]]);
  });
});

describe("the layout across a relaunch", () => {
  it("is kept once the launch's own has been read, and comes back", () => {
    const before = freshStore();
    before.restore(OPEN);
    before.select("c", false);
    before.moveToOther("left", "c", OPEN);
    before.setRatio(0.3, true);

    const after = freshStore();
    after.restore(OPEN);
    expect(after.isSplit()).toBe(true);
    expect(after.focused()).toBe("right");
    expect(after.activeIn("right")).toBe("c");
    expect(after.ratio()).toBe(0.3);
    expect(after.tabsIn("left", OPEN)).toEqual(["a", "b"]);
  });

  it("writes nothing before it has been read", () => {
    const panes = freshStore();
    panes.select("c", false);
    panes.moveToOther("left", "c", OPEN);
    expect(localStorage.getItem(PANES_STORAGE_KEY)).toBeNull();
  });

  it("comes back as one pane when the right pane's tabs are gone", () => {
    const before = freshStore();
    before.restore(OPEN);
    before.select("c", false);
    before.moveToOther("left", "c", OPEN);

    const after = freshStore();
    after.restore(["a", "b"]);
    expect(after.isSplit()).toBe(false);
    expect(after.focused()).toBe("left");
    expect(after.activeIn("left")).toBe("b");
  });

  it("keeps a drag's ratio only once it ends", () => {
    const panes = freshStore();
    panes.restore(OPEN);
    panes.setRatio(0.7, false);
    expect(JSON.parse(localStorage.getItem(PANES_STORAGE_KEY)!).ratio).toBe(0.5);
    panes.setRatio(0.7, true);
    expect(JSON.parse(localStorage.getItem(PANES_STORAGE_KEY)!).ratio).toBe(0.7);
  });
});
