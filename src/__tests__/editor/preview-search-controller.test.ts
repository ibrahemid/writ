import { describe, it, expect, vi } from "vitest";
import { createPreviewSearchController } from "../../editor/search/preview-search-controller";
import type { SearchTerm } from "../../editor/search/types";

function term(overrides: Partial<SearchTerm> = {}): SearchTerm {
  return { query: "x", caseSensitive: false, wholeWord: false, regexp: false, replace: "", ...overrides };
}

function setup() {
  const posted: Record<string, unknown>[] = [];
  const onUpdate = vi.fn();
  const ctrl = createPreviewSearchController({
    post: (msg) => posted.push(msg),
    onUpdate,
  });
  return { ctrl, posted, onUpdate };
}

describe("createPreviewSearchController", () => {
  it("posts a find command mapping the search term flags", () => {
    const { ctrl, posted } = setup();
    ctrl.setQuery(term({ query: "foo", caseSensitive: true, wholeWord: true, regexp: false }));
    expect(posted).toContainEqual({
      type: "find",
      query: "foo",
      caseSensitive: true,
      wholeWord: true,
      regexp: false,
    });
  });

  it("posts navigation commands for next and previous", () => {
    const { ctrl, posted } = setup();
    ctrl.next();
    ctrl.previous();
    expect(posted).toContainEqual({ type: "findNext" });
    expect(posted).toContainEqual({ type: "findPrev" });
  });

  it("exposes the latest result through matchState and calls onUpdate", () => {
    const { ctrl, onUpdate } = setup();
    ctrl.applyResult({ current: 2, total: 5, capped: false, ticks: [{ fraction: 0.1 }] });
    expect(ctrl.matchState()).toEqual({ current: 2, total: 5, capped: false });
    expect(onUpdate).toHaveBeenCalledTimes(1);
  });

  it("returns cached ticks limited to the requested count", () => {
    const { ctrl } = setup();
    ctrl.applyResult({
      current: 1,
      total: 3,
      capped: false,
      ticks: [{ fraction: 0.1 }, { fraction: 0.2 }, { fraction: 0.3 }],
    });
    expect(ctrl.matchPositions(2)).toEqual([{ fraction: 0.1 }, { fraction: 0.2 }]);
  });

  it("clears highlights and resets state", () => {
    const { ctrl, posted } = setup();
    ctrl.applyResult({ current: 1, total: 3, capped: false, ticks: [{ fraction: 0.1 }] });
    ctrl.clear();
    expect(posted).toContainEqual({ type: "findClear" });
    expect(ctrl.matchState()).toEqual({ current: 0, total: 0, capped: false });
    expect(ctrl.matchPositions(10)).toEqual([]);
  });

  it("treats replace as a no-op (preview is read-only)", () => {
    const { ctrl, posted } = setup();
    ctrl.replaceCurrent();
    ctrl.replaceAll();
    expect(posted).toEqual([]);
  });

  it("re-posts the last query on reapply so a reload can restore highlights", () => {
    const { ctrl, posted } = setup();
    ctrl.setQuery(term({ query: "foo", caseSensitive: true }));
    posted.length = 0;
    ctrl.reapply();
    expect(posted).toContainEqual({
      type: "find",
      query: "foo",
      caseSensitive: true,
      wholeWord: false,
      regexp: false,
    });
  });

  it("does not re-post on reapply when no query is active", () => {
    const { ctrl, posted } = setup();
    ctrl.reapply();
    expect(posted).toEqual([]);
  });

  it("resets state synchronously for an empty query", () => {
    const { ctrl } = setup();
    ctrl.applyResult({ current: 1, total: 3, capped: false, ticks: [{ fraction: 0.1 }] });
    ctrl.setQuery(term({ query: "" }));
    expect(ctrl.matchState()).toEqual({ current: 0, total: 0, capped: false });
  });
});
