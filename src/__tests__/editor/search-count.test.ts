import { describe, it, expect } from "vitest";
import { EditorState, EditorSelection } from "@codemirror/state";
import { SearchQuery } from "@codemirror/search";
import { countMatches } from "../../editor/search/count";

function stateOf(doc: string, selection?: { from: number; to: number }) {
  return EditorState.create({
    doc,
    selection: selection
      ? EditorSelection.single(selection.from, selection.to)
      : EditorSelection.single(0),
  });
}

function query(search: string, opts: Partial<SearchQuery> = {}) {
  return new SearchQuery({ search, ...opts });
}

describe("countMatches", () => {
  it("returns zero for an empty query", () => {
    expect(countMatches(stateOf("foo foo"), query(""))).toEqual({
      current: 0,
      total: 0,
      capped: false,
    });
  });

  it("counts every non-overlapping occurrence", () => {
    const r = countMatches(stateOf("foo bar foo baz foo"), query("foo"));
    expect(r.total).toBe(3);
    expect(r.current).toBe(0);
    expect(r.capped).toBe(false);
  });

  it("reports the 1-based index of the match under the selection", () => {
    // second "foo" spans [8,11]
    const r = countMatches(stateOf("foo bar foo baz foo", { from: 8, to: 11 }), query("foo"));
    expect(r.total).toBe(3);
    expect(r.current).toBe(2);
  });

  it("honours case sensitivity", () => {
    const insensitive = countMatches(stateOf("Foo foo FOO"), query("foo"));
    expect(insensitive.total).toBe(3);
    const sensitive = countMatches(stateOf("Foo foo FOO"), query("foo", { caseSensitive: true }));
    expect(sensitive.total).toBe(1);
  });

  it("honours whole-word matching", () => {
    const partial = countMatches(stateOf("foo foobar food"), query("foo"));
    expect(partial.total).toBe(3);
    const whole = countMatches(stateOf("foo foobar food"), query("foo", { wholeWord: true }));
    expect(whole.total).toBe(1);
  });

  it("supports regular expressions with the regexp flag", () => {
    const r = countMatches(stateOf("a1 b2 c3 dd"), query("[a-z][0-9]", { regexp: true }));
    expect(r.total).toBe(3);
  });

  it("returns zero for an invalid regular expression", () => {
    const r = countMatches(stateOf("anything"), query("(", { regexp: true }));
    expect(r).toEqual({ current: 0, total: 0, capped: false });
  });

  it("caps the total and flags it", () => {
    const r = countMatches(stateOf("x".repeat(50)), query("x"), 10);
    expect(r.total).toBe(10);
    expect(r.capped).toBe(true);
  });
});
