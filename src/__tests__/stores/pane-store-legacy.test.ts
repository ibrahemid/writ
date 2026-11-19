import { describe, it, expect, beforeEach } from "vitest";
import { createRoot } from "solid-js";
import { createPaneStore, PANES_STORAGE_KEY } from "../../stores/window/pane-store";
import { LEGACY_PANES_STORAGE_KEY } from "../../lib/legacy-names";

// The pane layout an earlier build kept under the old key comes back after the
// upgrade (ADR-059 decision 12).

const OPEN = ["a", "b", "c"];

function stored(ratio: number, right: string[]): string {
  return JSON.stringify({
    v: 1,
    ratio,
    focused: "right",
    right,
    both: [],
    active: { left: "a", right: right[0] ?? null },
  });
}

function freshStore() {
  return createRoot(() => createPaneStore());
}

beforeEach(() => {
  localStorage.clear();
});

describe("a pane layout kept under the old key", () => {
  it("comes back and moves to the current key", () => {
    localStorage.setItem(LEGACY_PANES_STORAGE_KEY, stored(0.3, ["c"]));
    const panes = freshStore();
    panes.restore(OPEN);
    expect(panes.isSplit()).toBe(true);
    expect(panes.ratio()).toBe(0.3);
    expect(panes.activeIn("right")).toBe("c");
    expect(localStorage.getItem(LEGACY_PANES_STORAGE_KEY)).toBeNull();
    expect(JSON.parse(localStorage.getItem(PANES_STORAGE_KEY)!).ratio).toBe(0.3);
  });

  it("gives way to the current key when both are present", () => {
    localStorage.setItem(LEGACY_PANES_STORAGE_KEY, stored(0.3, ["c"]));
    localStorage.setItem(PANES_STORAGE_KEY, stored(0.6, ["b"]));
    const panes = freshStore();
    panes.restore(OPEN);
    expect(panes.ratio()).toBe(0.6);
    expect(panes.activeIn("right")).toBe("b");
    expect(localStorage.getItem(LEGACY_PANES_STORAGE_KEY)).toBeNull();
  });
});
