import { describe, it, expect, beforeEach, vi } from "vitest";

const KEY = "writ.tree.expanded.v1";

async function freshStore() {
  vi.resetModules();
  return (await import("../../stores/global/tree-expansion")).treeExpansionStore;
}

beforeEach(() => {
  localStorage.clear();
});

describe("the folders a tree has open", () => {
  it("are kept per tree root", async () => {
    const store = await freshStore();
    store.setExpanded("/a", "/a/src", true);

    expect(store.isExpanded("/a", "/a/src")).toBe(true);
    expect(store.isExpanded("/b", "/a/src")).toBe(false);

    store.setExpanded("/a", "/a/src", false);
    expect(store.isExpanded("/a", "/a/src")).toBe(false);
  });

  it("come back after a relaunch (NV-18)", async () => {
    const before = await freshStore();
    before.setExpanded("/a", "/a/src", true);
    before.setExpanded("/a", "/a/docs", true);
    before.setExpanded("/a", "/a/docs", false);

    const after = await freshStore();
    expect(after.isExpanded("/a", "/a/src")).toBe(true);
    expect(after.isExpanded("/a", "/a/docs")).toBe(false);
  });

  it("start folded when what was kept cannot be read", async () => {
    localStorage.setItem(KEY, "{not json");
    const store = await freshStore();
    expect(store.isExpanded("/a", "/a/src")).toBe(false);
    store.setExpanded("/a", "/a/src", true);
    expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual({ "/a": ["/a/src"] });
  });

  it("keep the twenty roots changed last", async () => {
    const store = await freshStore();
    for (let n = 0; n < 21; n++) store.setExpanded(`/r${n}`, `/r${n}/x`, true);

    expect(store.isExpanded("/r0", "/r0/x")).toBe(false);
    expect(store.isExpanded("/r20", "/r20/x")).toBe(true);
    expect(Object.keys(JSON.parse(localStorage.getItem(KEY)!))).toHaveLength(20);
  });
});
