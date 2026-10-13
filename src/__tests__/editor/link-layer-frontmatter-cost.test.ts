import { describe, it, expect, vi, afterEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";

const fences = vi.hoisted(() => ({ calls: 0 }));

vi.mock("../../lib/frontmatter", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/frontmatter")>();
  return {
    ...actual,
    isFrontmatterFence: (line: string) => {
      fences.calls += 1;
      return actual.isFrontmatterFence(line);
    },
  };
});

import { findLinkTargets, isInsideFrontmatter, linkLayer, type LinkDeps } from "../../editor/link-layer";

const BODY_PARAGRAPHS = 20_000;

function body(lines: number): string {
  const out: string[] = [];
  for (let i = 0; i < lines; i++) out.push(`line ${i} of the body with [[Note ${i}]] in it`);
  return out.join("\n");
}

// One paragraph per line. Markdown parses a paragraph whole, so a body of one
// paragraph thousands of lines long costs jsdom a second per keystroke at its
// end, with or without the link layer.
function paragraphs(count: number): string {
  const out: string[] = [];
  for (let i = 0; i < count; i++) out.push(`line ${i} of the body with [[Note ${i}]] in it`);
  return out.join("\n\n");
}

const deps: LinkDeps = { openUrl: () => {}, openWorkspaceFile: () => {}, openNoteLink: () => true };

let view: EditorView | null = null;

function mount(doc: string): EditorView {
  view = new EditorView({
    state: EditorState.create({
      doc,
      extensions: [markdown({ base: markdownLanguage }), linkLayer(deps)],
    }),
    parent: document.body,
  });
  return view;
}

function type(target: EditorView, at: number, text: string): void {
  target.dispatch({ changes: { from: at, insert: text }, userEvent: "input.type" });
}

function wikilinksIn(target: EditorView, from: number, to: number): string[] {
  return findLinkTargets(target.state, from, to)
    .filter((range) => range.kind === "wikilink")
    .map((range) => target.state.doc.sliceString(range.from, range.to));
}

afterEach(() => {
  view?.destroy();
  view = null;
  fences.calls = 0;
});

describe("the frontmatter extent on a keystroke", () => {
  it("does not walk the document again when the leading --- is never closed", () => {
    const target = mount(`---\n${paragraphs(BODY_PARAGRAPHS)}`);
    const end = target.state.doc.length;
    fences.calls = 0;

    const perKeystroke: number[] = [];
    for (let i = 0; i < 10; i++) {
      const before = fences.calls;
      type(target, end + i, "x");
      perKeystroke.push(fences.calls - before);
    }

    expect(Math.max(...perKeystroke)).toBeLessThan(50);
  });

  it("does not walk the document again for a keystroke below a closed block", () => {
    const target = mount(`---\ntitle: x\n---\n${paragraphs(BODY_PARAGRAPHS)}`);
    const end = target.state.doc.length;
    fences.calls = 0;

    for (let i = 0; i < 10; i++) type(target, end + i, "x");

    expect(fences.calls).toBeLessThan(50);
  });

  it("closes the block when a closing fence is typed far below the opening one", () => {
    const target = mount(`---\nsee: "[[Inside]]"\n${body(200)}`);
    expect(isInsideFrontmatter(target.state, 8)).toBe(false);

    const line = target.state.doc.line(120);
    type(target, line.to, "\n---");

    expect(isInsideFrontmatter(target.state, 8)).toBe(true);
    expect(wikilinksIn(target, 0, 20)).toEqual([]);
    const after = target.state.doc.line(122);
    expect(isInsideFrontmatter(target.state, after.from)).toBe(false);
    expect(wikilinksIn(target, after.from, after.to)).toEqual(["Note 118"]);
  });

  it("opens the block again when its closing fence is removed", () => {
    const target = mount(`---\nsee: "[[Inside]]"\n---\n${body(50)}`);
    expect(isInsideFrontmatter(target.state, 8)).toBe(true);

    const closing = target.state.doc.line(3);
    target.dispatch({ changes: { from: closing.from, to: closing.to, insert: "x" } });

    expect(isInsideFrontmatter(target.state, 8)).toBe(false);
    expect(wikilinksIn(target, 0, 20)).toEqual(["Inside"]);
  });

  it("follows the first line in and out of being a fence", () => {
    const target = mount(`title\nsee: "[[Inside]]"\n---\n${body(50)}`);
    expect(isInsideFrontmatter(target.state, 8)).toBe(false);

    target.dispatch({ changes: { from: 0, to: 5, insert: "---" } });
    expect(isInsideFrontmatter(target.state, 6)).toBe(true);

    target.dispatch({ changes: { from: 0, to: 3, insert: "title" } });
    expect(isInsideFrontmatter(target.state, 8)).toBe(false);
  });

  it("finds the first closing fence when one is typed above the old one", () => {
    const target = mount(`---\na: 1\nb: 2\n---\n${body(10)}`);
    const old = target.state.doc.line(4).to + 1;
    expect(isInsideFrontmatter(target.state, old - 1)).toBe(true);

    const line = target.state.doc.line(2);
    type(target, line.to, "\n---");

    expect(isInsideFrontmatter(target.state, target.state.doc.line(3).from)).toBe(true);
    expect(isInsideFrontmatter(target.state, target.state.doc.line(4).from)).toBe(false);
  });

  it("matches a fresh scan after every edit of a sequence", () => {
    const target = mount(`---\na: [[A]]\n${body(30)}`);
    const edits: Array<{ from: number; to?: number; insert: string }> = [
      { from: 0, insert: "x" },
      { from: 0, to: 1, insert: "" },
      { from: 10, insert: "\n---\n" },
      { from: 12, to: 14, insert: "" },
      { from: 3, insert: "\n---" },
      { from: 0, to: 4, insert: "" },
      { from: 0, insert: "---\n" },
    ];
    for (const edit of edits) {
      target.dispatch({ changes: edit });
      const fresh = EditorState.create({ doc: target.state.doc.toString() });
      for (let pos = 0; pos < Math.min(target.state.doc.length, 80); pos += 7) {
        expect(isInsideFrontmatter(target.state, pos)).toBe(isInsideFrontmatter(fresh, pos));
      }
    }
  });
});
