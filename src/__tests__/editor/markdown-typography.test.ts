import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { syntaxTree } from "@codemirror/language";
import { buildMarkdownDecorations, markdownTypographyPlugin, type DecorationSpec } from "../../editor/markdown-typography";

// ─── Helpers ──────────────────────────────────────────────────────────────

function buildForDoc(
  doc: string,
  cursorPositions: number[] = [],
): DecorationSpec[] {
  const state = EditorState.create({ doc, extensions: [markdown()] });
  const tree = syntaxTree(state);
  const cursors = new ReadonlySet(cursorPositions);
  return buildMarkdownDecorations(
    (from, to, cb) => tree.iterate({ from, to, enter: cb }),
    (pos) => state.doc.lineAt(pos),
    cursors,
    0,
    doc.length,
  );
}

// Minimal ReadonlySet shim for test environment.
class ReadonlySet<T> extends Set<T> implements ReadonlySet<T> {}

// ─── Heading line decorations ─────────────────────────────────────────────

describe("heading line decorations", () => {
  it("emits a line decoration for ATXHeading1", () => {
    const specs = buildForDoc("# Hello\n");
    const lineSpec = specs.find(
      (s) => s.from === 0 && s.to === 0 && (s.decoration as unknown as { spec: { class: string } }).spec.class === "cm-line-md-h1",
    );
    expect(lineSpec).toBeDefined();
  });

  it("emits correct classes for h2-h6", () => {
    const doc = "## H2\n### H3\n#### H4\n##### H5\n###### H6\n";
    const specs = buildForDoc(doc);
    const classes = specs
      .filter((s) => s.from === s.to)
      .map((s) => (s.decoration as unknown as { spec: { class: string } }).spec.class);
    expect(classes).toContain("cm-line-md-h2");
    expect(classes).toContain("cm-line-md-h3");
    expect(classes).toContain("cm-line-md-h4");
    expect(classes).toContain("cm-line-md-h5");
    expect(classes).toContain("cm-line-md-h6");
  });

  it("does not emit heading decorations for plain text", () => {
    const specs = buildForDoc("plain text\n");
    const headings = specs.filter(
      (s) => (s.decoration as unknown as { spec: { class: string } }).spec?.class?.startsWith("cm-line-md-h"),
    );
    expect(headings).toHaveLength(0);
  });
});

// ─── Marker hide/reveal behaviour ─────────────────────────────────────────

describe("syntax marker hiding", () => {
  it("replaces heading markers on inactive lines", () => {
    // Line 0 starts at pos 0. Cursor is on line 1 (pos 8+).
    const doc = "# Hello\nsome text\n";
    const state = EditorState.create({ doc, extensions: [markdown()] });
    const cursorOnLine2 = state.doc.line(2).from;
    const specs = buildForDoc(doc, [cursorOnLine2]);
    // HeaderMark '#' + space = positions 0..2 → should be replaced
    const replaces = specs.filter(
      (s) => (s.decoration as unknown as { spec: Record<string, unknown> }).spec?.widget === undefined &&
             s.from !== s.to &&
             s.decoration.spec !== undefined &&
             // Decoration.replace has a widget of null or undefined and no class
             (s.decoration as unknown as { spec: { class?: string } }).spec.class === undefined,
    );
    // At least one replace decoration exists for the marker
    expect(replaces.length).toBeGreaterThan(0);
  });

  it("does not replace heading markers on the active line", () => {
    const doc = "# Hello\n";
    // Cursor at position 2 (on the heading line)
    const specs = buildForDoc(doc, [2]);
    // No replace decorations for line 0 markers
    const replaces = specs.filter(
      (s) => s.from >= 0 && s.to <= 7 &&
             (s.decoration as unknown as { spec: { class?: string } }).spec?.class === undefined &&
             s.from !== s.to,
    );
    expect(replaces).toHaveLength(0);
  });

  it("replaces bold markers on inactive lines", () => {
    const doc = "**bold**\ncursor here\n";
    const state = EditorState.create({ doc, extensions: [markdown()] });
    const cursorOnLine2 = state.doc.line(2).from;
    const specs = buildForDoc(doc, [cursorOnLine2]);
    const replaces = specs.filter(
      (s) => s.from >= 0 && s.to <= doc.indexOf("\n") &&
             (s.decoration as unknown as { spec: { class?: string } }).spec?.class === undefined &&
             s.from !== s.to,
    );
    expect(replaces.length).toBeGreaterThan(0);
  });

  it("does not replace bold markers on the active line", () => {
    const doc = "**bold**\n";
    // Cursor inside the bold span
    const specs = buildForDoc(doc, [4]);
    const replaces = specs.filter(
      (s) => s.from >= 0 && s.to <= doc.indexOf("\n") &&
             (s.decoration as unknown as { spec: { class?: string } }).spec?.class === undefined &&
             s.from !== s.to,
    );
    expect(replaces).toHaveLength(0);
  });
});

// ─── Inline mark decorations ──────────────────────────────────────────────

describe("inline mark decorations", () => {
  it("marks strong emphasis with cm-md-strong", () => {
    const doc = "**bold text**\n";
    const specs = buildForDoc(doc);
    const strong = specs.find(
      (s) => (s.decoration as unknown as { spec: { class: string } }).spec?.class === "cm-md-strong",
    );
    expect(strong).toBeDefined();
    expect(strong!.from).toBeLessThan(strong!.to);
  });

  it("marks emphasis with cm-md-em", () => {
    const doc = "*italic*\n";
    const specs = buildForDoc(doc);
    const em = specs.find(
      (s) => (s.decoration as unknown as { spec: { class: string } }).spec?.class === "cm-md-em",
    );
    expect(em).toBeDefined();
  });

  it("marks inline code with cm-md-code", () => {
    const doc = "use `code` here\n";
    const specs = buildForDoc(doc);
    const code = specs.find(
      (s) => (s.decoration as unknown as { spec: { class: string } }).spec?.class === "cm-md-code",
    );
    expect(code).toBeDefined();
    // Span should include the backtick delimiters as the mark wraps the whole InlineCode node
    expect(code!.from).toBe(doc.indexOf("`"));
    expect(code!.to).toBe(doc.indexOf("`") + "`code`".length);
  });

  it("marks strikethrough with cm-md-strike when GFM base is used", () => {
    // Strikethrough requires GFM extensions (markdownLanguage base).
    const doc = "~~struck~~\n";
    const state = EditorState.create({
      doc,
      extensions: [markdown({ base: markdownLanguage })],
    });
    const tree = syntaxTree(state);
    const specs = buildMarkdownDecorations(
      (from, to, cb) => tree.iterate({ from, to, enter: cb }),
      (pos) => state.doc.lineAt(pos),
      new ReadonlySet([]),
      0,
      doc.length,
    );
    const strike = specs.find(
      (s) => (s.decoration as unknown as { spec: { class: string } }).spec?.class === "cm-md-strike",
    );
    expect(strike).toBeDefined();
  });
});

// ─── Non-overlapping invariant ────────────────────────────────────────────

describe("decoration non-overlap invariant", () => {
  it("produces no overlapping replace decorations", () => {
    const doc = [
      "# Heading",
      "**bold** and *italic*",
      "> blockquote with `code`",
      "[link](url)",
      "~~strike~~",
      "",
    ].join("\n");

    const specs = buildForDoc(doc);
    const replaces = specs.filter(
      (s) => (s.decoration as unknown as { spec: { class?: string } }).spec?.class === undefined && s.from !== s.to,
    );

    // Check pairwise non-overlap.
    for (let i = 0; i < replaces.length; i++) {
      for (let j = i + 1; j < replaces.length; j++) {
        const a = replaces[i];
        const b = replaces[j];
        const overlaps = a.from < b.to && a.to > b.from;
        if (overlaps) {
          throw new Error(
            `Overlapping replace decorations: [${a.from},${a.to}) and [${b.from},${b.to})`,
          );
        }
      }
    }
  });

  it("produces decorations sorted by from position", () => {
    const doc = "# Heading\n**bold** and *italic*\n> quote\n`code`\n";
    const specs = buildForDoc(doc);
    for (let i = 1; i < specs.length; i++) {
      expect(specs[i].from).toBeGreaterThanOrEqual(specs[i - 1].from);
    }
  });
});

// ─── Visible range scoping ────────────────────────────────────────────────

describe("visible range scoping", () => {
  it("omits decorations outside the visible range", () => {
    const doc = "# H1 visible\n# H2 invisible\n";
    const state = EditorState.create({ doc, extensions: [markdown()] });
    const tree = syntaxTree(state);
    const line1End = state.doc.line(1).to;

    const specs = buildMarkdownDecorations(
      (from, to, cb) => tree.iterate({ from, to, enter: cb }),
      (pos) => state.doc.lineAt(pos),
      new ReadonlySet([]),
      0,
      line1End,
    );

    // Should have a heading for line 1 (pos 0) but not line 2
    const headings = specs.filter(
      (s) => s.from === s.to && (s.decoration as unknown as { spec: { class: string } }).spec?.class?.startsWith("cm-line-md-h"),
    );
    expect(headings.some((s) => s.from === 0)).toBe(true);
    // Line 2 starts at line1End + 1; no decoration for it
    expect(headings.some((s) => s.from > line1End)).toBe(false);
  });
});

// ─── Blockquote ───────────────────────────────────────────────────────────

describe("blockquote decoration", () => {
  it("applies cm-md-blockquote mark to blockquote span", () => {
    const doc = "> quoted text\n";
    const specs = buildForDoc(doc);
    const bq = specs.find(
      (s) => (s.decoration as unknown as { spec: { class: string } }).spec?.class === "cm-md-blockquote",
    );
    expect(bq).toBeDefined();
    expect(bq!.from).toBe(0);
  });
});

// ─── ViewPlugin smoke tests ───────────────────────────────────────────────
// Exercises the runtime path (ViewPlugin + Decoration.set) that pure-function
// tests cannot reach. jsdom is sufficient — CM needs no real layout.

describe("markdownTypographyPlugin runtime", () => {
  it("constructs without throwing on a markdown document", () => {
    const state = EditorState.create({
      doc: "# H1\n**bold** and `code`\n> quote\n[x](y)\n",
      extensions: [markdown(), markdownTypographyPlugin],
    });
    const view = new EditorView({ state });
    expect(() =>
      view.dispatch({ selection: { anchor: 0 } }),
    ).not.toThrow();
    view.destroy();
  });

  it("does not throw when cursor moves through various syntax nodes", () => {
    const doc = "# Heading\n~~strike~~ *em* **bold** `code`\n> blockquote\n[link](url)\n";
    const state = EditorState.create({
      doc,
      extensions: [markdown({ base: markdownLanguage }), markdownTypographyPlugin],
    });
    const view = new EditorView({ state });
    for (let pos = 0; pos <= doc.length; pos += 3) {
      expect(() => view.dispatch({ selection: { anchor: pos } })).not.toThrow();
    }
    view.destroy();
  });
});
