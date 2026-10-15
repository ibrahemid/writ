import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { ensureSyntaxTree } from "@codemirror/language";
import {
  MARKDOWN_EXTENSIONS,
  blockMathLength,
  mathSource,
  mathSpanEnd,
} from "../../editor/markdown-math";

function nodes(doc: string): Array<[string, string]> {
  const state = EditorState.create({
    doc,
    extensions: [markdown({ base: markdownLanguage, extensions: MARKDOWN_EXTENSIONS })],
  });
  const tree = ensureSyntaxTree(state, doc.length, 5000)!;
  const found: Array<[string, string]> = [];
  tree.iterate({
    enter: (node) => {
      if (node.name === "InlineMath" || node.name === "BlockMath") {
        found.push([node.name, doc.slice(node.from, node.to)]);
      }
    },
  });
  return found;
}

function spanEnd(text: string, start = 0): number {
  return mathSpanEnd((pos) => (pos < text.length ? text.charCodeAt(pos) : -1), start, text.length);
}

describe("math spans, by the preview parser's rules", () => {
  it("reads inline math", () => {
    expect(nodes("Area is $\\pi r^2$ here.\n")).toEqual([["InlineMath", "$\\pi r^2$"]]);
  });

  it("leaves currency as text", () => {
    expect(nodes("It costs $5 and $10 today.\n")).toEqual([]);
    expect(nodes("A lone $ sign.\n")).toEqual([]);
  });

  it("needs no whitespace inside either delimiter", () => {
    expect(nodes("a $ x$ b\n")).toEqual([]);
    expect(nodes("a $x $ b\n")).toEqual([]);
    expect(spanEnd("$$")).toBe(-1);
  });

  it("keeps a dollar inside braces in the expression", () => {
    expect(nodes("$\\text{a $b$}$\n")).toEqual([["InlineMath", "$\\text{a $b$}$"]]);
  });

  it("finds no math in a code span, a fence or after an escape", () => {
    expect(nodes("Use `$x$` here.\n")).toEqual([]);
    expect(nodes("```\n$x$\n```\n")).toEqual([]);
    expect(nodes("Not \\$x$ math.\n")).toEqual([]);
  });

  it("reads math inside a table cell", () => {
    expect(nodes("| a | b |\n|---|---|\n| $x$ | 2 |\n")).toEqual([["InlineMath", "$x$"]]);
  });

  it("makes a paragraph of one display span a block", () => {
    expect(nodes("$$\nx = 1\n$$\n")).toEqual([["BlockMath", "$$\nx = 1\n$$"]]);
    expect(nodes("intro\n\n$$x$$\n")).toEqual([["BlockMath", "$$x$$"]]);
  });

  it("ends the block on its closing line", () => {
    const doc = "$$\nx\n$$\nafter\n";
    expect(nodes(doc)).toEqual([["BlockMath", "$$\nx\n$$"]]);
  });

  it("keeps display math with text around it inline", () => {
    expect(nodes("so $$x$$ then\n")).toEqual([["InlineMath", "$$x$$"]]);
  });

  it("leaves an unclosed display opener as text", () => {
    expect(nodes("$$\nx\n\nmore\n")).toEqual([]);
    expect(blockMathLength("$$ $$")).toBe(-1);
    expect(blockMathLength("$$a$b$$")).toBe(-1);
  });

  it("names the expression and its mode", () => {
    expect(mathSource("$x$")).toEqual({ tex: "x", display: false });
    expect(mathSource("$$\n x \n$$")).toEqual({ tex: "x", display: true });
  });
});
