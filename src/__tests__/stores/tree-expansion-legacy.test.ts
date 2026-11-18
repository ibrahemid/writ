import { describe, it, expect, beforeEach, vi } from "vitest";
import { LEGACY_TREE_EXPANSION_STORAGE_KEY } from "../../lib/legacy-names";

// The open folders an earlier build kept under the old key come back after the
// upgrade (ADR-059 decision 12).

const KEY = "jotra.tree.expanded.v1";

async function freshStore() {
  vi.resetModules();
  return (await import("../../stores/global/tree-expansion")).treeExpansionStore;
}

beforeEach(() => {
  localStorage.clear();
});

describe("open folders kept under the old key", () => {
  it("come back and move to the current key", async () => {
    localStorage.setItem(LEGACY_TREE_EXPANSION_STORAGE_KEY, JSON.stringify({ "/a": ["/a/src"] }));
    const store = await freshStore();
    expect(store.isExpanded("/a", "/a/src")).toBe(true);
    expect(localStorage.getItem(LEGACY_TREE_EXPANSION_STORAGE_KEY)).toBeNull();
    expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual({ "/a": ["/a/src"] });
  });

  it("give way to the current key when both are present", async () => {
    localStorage.setItem(LEGACY_TREE_EXPANSION_STORAGE_KEY, JSON.stringify({ "/a": ["/a/old"] }));
    localStorage.setItem(KEY, JSON.stringify({ "/a": ["/a/new"] }));
    const store = await freshStore();
    expect(store.isExpanded("/a", "/a/new")).toBe(true);
    expect(store.isExpanded("/a", "/a/old")).toBe(false);
    expect(localStorage.getItem(LEGACY_TREE_EXPANSION_STORAGE_KEY)).toBeNull();
  });
});
