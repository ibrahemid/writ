import { describe, it, expect, vi } from "vitest";
import { spellingMenuItems } from "../../editor/spelling-menu";
import { computeChosenFix } from "../../editor/spelling";
import type { SpellingEntry } from "../../editor/spelling";

function entry(overrides: Partial<SpellingEntry> = {}): SpellingEntry {
  return {
    from: 3,
    to: 6,
    word: "teh",
    message: "Possible spelling mistake",
    kind: "spelling",
    suggestions: ["the", "tea"],
    confident: true,
    ...overrides,
  };
}

describe("spelling popover rows", () => {
  it("puts the corrections first so one click fixes the word", () => {
    const items = spellingMenuItems(entry(), {
      apply: vi.fn(),
      addToDictionary: vi.fn(),
    });
    expect(items.map((i) => i.label)).toEqual(["the", "tea", 'Add "teh" to dictionary']);
  });

  it("applies the single correction the user clicked", () => {
    const apply = vi.fn();
    const e = entry();
    const items = spellingMenuItems(e, { apply, addToDictionary: vi.fn() });
    items[1].action();
    expect(apply).toHaveBeenCalledWith(e, "tea");
  });

  it("offers a per-word dictionary action", () => {
    const addToDictionary = vi.fn();
    const items = spellingMenuItems(entry(), { apply: vi.fn(), addToDictionary });
    items[items.length - 1].action();
    expect(addToDictionary).toHaveBeenCalledWith("teh");
  });

  it("still offers the dictionary action when there is nothing to suggest", () => {
    const items = spellingMenuItems(entry({ suggestions: [] }), {
      apply: vi.fn(),
      addToDictionary: vi.fn(),
    });
    expect(items[0].label).toBe("No suggestions");
    expect(items[0].disabled).toBe(true);
    expect(items[1].label).toBe('Add "teh" to dictionary');
  });
});

describe("single-word fix", () => {
  const docSlice = (doc: string) => (from: number, to: number) => doc.slice(from, to);

  it("replaces exactly the flagged range", () => {
    const fix = computeChosenFix(entry(), docSlice("in teh middle"), "the");
    expect(fix).toEqual({ from: 3, to: 6, insert: "the" });
  });

  it("accepts a correction the batch fixer would skip", () => {
    // `computeFixChanges` only ever applies the first suggestion, and only for
    // confident entries. An explicit pick is neither of those.
    const fix = computeChosenFix(entry({ confident: false }), docSlice("in teh middle"), "tea");
    expect(fix).toEqual({ from: 3, to: 6, insert: "tea" });
  });

  it("refuses to write through a stale offset", () => {
    // The word moved or was already edited: applying here would corrupt text.
    expect(computeChosenFix(entry(), docSlice("in cat middle"), "the")).toBeNull();
  });
});
