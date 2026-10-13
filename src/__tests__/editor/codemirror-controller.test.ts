import { describe, it, expect, afterEach, vi } from "vitest";
import { EditorView } from "@codemirror/view";
import { EditorState } from "@codemirror/state";
import { search, searchPanelOpen } from "@codemirror/search";
import { createCodeMirrorSearchController } from "../../editor/search/codemirror-controller";
import type { SearchTerm } from "../../editor/search/types";
import { engineGivesUpOn } from "./fixtures/regexp-gives-up";

function term(overrides: Partial<SearchTerm> = {}): SearchTerm {
  return {
    query: "",
    caseSensitive: false,
    wholeWord: false,
    regexp: false,
    replace: "",
    ...overrides,
  };
}

function makeView(doc: string) {
  const state = EditorState.create({
    doc,
    extensions: [search({ top: true })],
  });
  const container = document.createElement("div");
  document.body.appendChild(container);
  return new EditorView({ state, parent: container });
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("CodeMirrorSearchController", () => {
  it("returns an empty match state when no view is bound", () => {
    const c = createCodeMirrorSearchController(() => null);
    expect(c.matchState()).toEqual({ current: 0, total: 0, capped: false });
    expect(c.matchPositions(50)).toEqual([]);
  });

  it("counts matches after setting a query", () => {
    const view = makeView("foo bar foo baz foo");
    const c = createCodeMirrorSearchController(() => view);
    c.setQuery(term({ query: "foo" }));
    expect(c.matchState().total).toBe(3);
    view.destroy();
  });

  it("navigates matches without opening the native panel", () => {
    const view = makeView("foo bar foo baz foo");
    const c = createCodeMirrorSearchController(() => view);
    c.setQuery(term({ query: "foo" }));

    c.next();
    expect(searchPanelOpen(view.state)).toBe(false);
    expect(c.matchState().current).toBe(1);

    c.next();
    expect(c.matchState().current).toBe(2);

    c.previous();
    expect(c.matchState().current).toBe(1);
    expect(searchPanelOpen(view.state)).toBe(false);
    view.destroy();
  });

  it("wraps to the first match after the last", () => {
    const view = makeView("foo foo");
    const c = createCodeMirrorSearchController(() => view);
    c.setQuery(term({ query: "foo" }));
    c.next();
    c.next();
    c.next();
    expect(c.matchState().current).toBe(1);
    view.destroy();
  });

  it("replaces the current match", () => {
    const view = makeView("foo bar foo");
    const c = createCodeMirrorSearchController(() => view);
    c.setQuery(term({ query: "foo", replace: "qux" }));
    c.next();
    c.replaceCurrent();
    expect(view.state.doc.toString()).toBe("qux bar foo");
    view.destroy();
  });

  it("replaces every match, including regexp capture references", () => {
    const view = makeView("2024-01 2025-12");
    const c = createCodeMirrorSearchController(() => view);
    c.setQuery(term({ query: "(\\d+)-(\\d+)", replace: "$2/$1", regexp: true }));
    c.replaceAll();
    expect(view.state.doc.toString()).toBe("01/2024 12/2025");
    view.destroy();
  });

  it("reports match tick fractions in document order", () => {
    const view = makeView("foo" + " ".repeat(97) + "foo");
    const c = createCodeMirrorSearchController(() => view);
    c.setQuery(term({ query: "foo" }));
    const ticks = c.matchPositions(50);
    expect(ticks.length).toBe(2);
    expect(ticks[0].fraction).toBeLessThan(ticks[1].fraction);
    expect(ticks[0].fraction).toBeGreaterThanOrEqual(0);
    expect(ticks[1].fraction).toBeLessThanOrEqual(1);
    view.destroy();
  });

  it("reports a regular expression that does not compile as invalid", () => {
    const view = makeView("anything (here)");
    const c = createCodeMirrorSearchController(() => view);
    c.setQuery(term({ query: "(", regexp: true }));
    const state = c.matchState();
    expect(state.invalid).toBe(true);
    expect(state.total).toBe(0);
    c.setQuery(term({ query: "\\(", regexp: true }));
    expect(c.matchState().invalid).toBeFalsy();
    expect(c.matchState().total).toBe(1);
    view.destroy();
  });

  it("never opens the native panel for a pattern that does not compile", () => {
    const view = makeView("anything (here)");
    const c = createCodeMirrorSearchController(() => view);
    c.setQuery(term({ query: "(", regexp: true }));
    c.next();
    c.previous();
    c.replaceCurrent();
    c.replaceAll();
    expect(searchPanelOpen(view.state)).toBe(false);
    expect(view.state.doc.toString()).toBe("anything (here)");
    view.destroy();
  });

  it("highlights every match in view before any navigation", () => {
    const view = makeView("foo bar foo baz foo");
    const c = createCodeMirrorSearchController(() => view);
    c.setQuery(term({ query: "foo" }));
    expect(view.dom.querySelectorAll(".cm-searchMatch")).toHaveLength(3);
    c.setQuery(term({ query: "ba[rz]", regexp: true }));
    expect(view.dom.querySelectorAll(".cm-searchMatch")).toHaveLength(2);
    c.clear();
    expect(view.dom.querySelectorAll(".cm-searchMatch")).toHaveLength(0);
    view.destroy();
  });

  it("clears the query and its matches", () => {
    const view = makeView("foo foo");
    const c = createCodeMirrorSearchController(() => view);
    c.setQuery(term({ query: "foo" }));
    expect(c.matchState().total).toBe(2);
    c.clear();
    expect(c.matchState().total).toBe(0);
    view.destroy();
  });

  it("searches for nothing when the query is only whitespace", () => {
    const view = makeView("a b  c\td");
    const c = createCodeMirrorSearchController(() => view);
    for (const query of [" ", "  ", "\t", " \t "]) {
      for (const regexp of [false, true]) {
        c.setQuery(term({ query, regexp }));
        expect(c.matchState().total, JSON.stringify({ query, regexp })).toBe(0);
        expect(view.dom.querySelectorAll(".cm-searchMatch"), JSON.stringify({ query, regexp })).toHaveLength(0);
      }
    }
    c.setQuery(term({ query: " c" }));
    expect(c.matchState().total).toBe(1);
    expect(view.dom.querySelectorAll(".cm-searchMatch")).toHaveLength(1);
    view.destroy();
  });

  it("moves nowhere for a pattern the regular expression engine gives up on", () => {
    engineGivesUpOn(["(\\D)+"]);
    const view = makeView("alpha beta");
    const c = createCodeMirrorSearchController(() => view);
    c.setQuery(term({ query: "(\\D)+", regexp: true }));
    expect(c.matchState()).toMatchObject({ total: 0, pending: false, failed: true });
    c.next();
    c.previous();
    c.replaceCurrent();
    c.replaceAll();
    expect(view.state.selection.main.head).toBe(0);
    expect(view.state.doc.toString()).toBe("alpha beta");
    expect(searchPanelOpen(view.state)).toBe(false);
    view.destroy();
  });
});
