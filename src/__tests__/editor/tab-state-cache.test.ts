import { describe, it, expect, vi } from "vitest";
import { EditorState } from "@codemirror/state";
import { TabStateCache, type TabState } from "../../editor/tab-state-cache";
import type { FileOpenMode } from "../../types/buffer";

function entry(length: number, kind: FileOpenMode["kind"] = "Normal"): TabState {
  return {
    state: EditorState.create({ doc: "x".repeat(length) }),
    scroll: null,
    mode: { kind } as FileOpenMode,
    longest: 0,
  };
}

describe("TabStateCache", () => {
  it("hands a kept state back once, and keeps count of what it holds", () => {
    const cache = new TabStateCache(1_000);
    cache.put("a", entry(100));
    expect(cache.size).toBe(100);
    expect(cache.take("a")?.state.doc.length).toBe(100);
    expect(cache.take("a")).toBeUndefined();
    expect(cache.size).toBe(0);
  });

  it("drops the least recently left large-tier tab first when over the budget", () => {
    const evicted = vi.fn();
    const cache = new TabStateCache(1_000, evicted);
    cache.put("normal-old", entry(300));
    cache.put("large-old", entry(300, "LargeFile"));
    cache.put("large-new", entry(300, "LargeFile"));
    cache.put("normal-new", entry(300));

    expect(evicted).toHaveBeenCalledWith("large-old");
    expect(cache.has("large-old")).toBe(false);
    expect(cache.has("normal-old")).toBe(true);
    expect(cache.size).toBeLessThanOrEqual(1_000);
  });

  it("drops normal tabs, oldest first, once no large one is left", () => {
    const evicted = vi.fn();
    const cache = new TabStateCache(1_000, evicted);
    cache.put("one", entry(400));
    cache.put("two", entry(400));
    cache.put("three", entry(400));
    expect(evicted.mock.calls.map(([id]) => id)).toEqual(["one"]);
    expect(cache.size).toBe(800);
  });

  it("keeps nothing for a tab whose document alone is over the budget", () => {
    const evicted = vi.fn();
    const cache = new TabStateCache(1_000, evicted);
    cache.put("huge", entry(2_000, "LargeFile"));
    expect(cache.has("huge")).toBe(false);
    expect(evicted).toHaveBeenCalledWith("huge");
    expect(cache.size).toBe(0);
  });

  it("follows a kept state that changed and drops the tabs that closed", () => {
    const cache = new TabStateCache(1_000);
    cache.put("a", entry(100));
    cache.put("b", entry(100));
    cache.update("a", EditorState.create({ doc: "y".repeat(250) }));
    expect(cache.size).toBe(350);
    cache.retain(new Set(["a"]));
    expect(cache.has("b")).toBe(false);
    expect(cache.size).toBe(250);
  });
});
