import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { findLinkTargets, linkHintText, linkRangeAt } from "../../editor/link-layer";

function md(doc: string): EditorState {
  return EditorState.create({ doc, extensions: [markdown({ base: markdownLanguage })] });
}

describe("the hint a link gives on hover", () => {
  it("says how to open a link, with the platform's modifier", () => {
    expect(linkHintText("url", null, true)).toBe("⌘-click to open");
    expect(linkHintText("path", null, false)).toBe("Ctrl+click to open");
    expect(linkHintText("wikilink", "resolved", true)).toBe("⌘-click to open");
  });

  it("says a link to a file that is not there creates it", () => {
    expect(linkHintText("wikilink", "missing", true)).toBe("⌘-click to create");
    expect(linkHintText("wikilink", "missing", false)).toBe("Ctrl+click to create");
  });

  it("does not promise a file before the link has resolved", () => {
    expect(linkHintText("wikilink", null, true)).toBe("⌘-click to open");
  });
});

describe("the link under the pointer", () => {
  it("finds a wikilink and a bare address", () => {
    const state = md("see [[Plan]] and https://example.com now");
    const ranges = findLinkTargets(state, 0, state.doc.length);
    expect(linkRangeAt(state, ranges, 7)?.kind).toBe("wikilink");
    expect(linkRangeAt(state, ranges, 25)?.kind).toBe("url");
    expect(linkRangeAt(state, ranges, 1)).toBeNull();
  });

  it("finds a Markdown link by its label", () => {
    const state = md("read [the docs](https://example.com/docs) first");
    const hit = linkRangeAt(state, [], 8);
    expect(hit?.target).toBe("https://example.com/docs");
  });
});
