import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { syntaxTree } from "@codemirror/language";
import {
  buildMarkdownDecorations,
  markdownTypographyPlugin,
  toggleTaskAt,
  type DecorationSpec,
} from "../../editor/markdown-typography";

// ─── Helpers ──────────────────────────────────────────────────────────────

function buildForDoc(
  doc: string,
  cursorPositions: number[] = [],
): DecorationSpec[] {
  const state = EditorState.create({ doc, extensions: [markdown({ base: markdownLanguage })] });
  const tree = syntaxTree(state);
  const cursors = new ReadonlySet(cursorPositions);
  return buildMarkdownDecorations(
    (from, to, cb) => tree.iterate({ from, to, enter: cb }),
    (pos) => state.doc.lineAt(pos),
    (from, to) => state.doc.sliceString(from, to),
    cursors,
    0,
    doc.length,
  );
}

function widgetSpecs(specs: DecorationSpec[]): DecorationSpec[] {
  return specs.filter(
    (s) => (s.decoration as unknown as { spec: { widget?: unknown } }).spec?.widget !== undefined,
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
      (from, to) => state.doc.sliceString(from, to),
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
      (from, to) => state.doc.sliceString(from, to),
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

// ─── Task checkboxes ──────────────────────────────────────────────────────

describe("task checkbox decorations", () => {
  it("replaces the task marker with a checkbox widget on inactive lines", () => {
    const doc = "- [ ] open\n- [x] done\ncursor here\n";
    const state = EditorState.create({ doc, extensions: [markdown({ base: markdownLanguage })] });
    const specs = buildForDoc(doc, [state.doc.line(3).from]);
    const widgets = widgetSpecs(specs).filter(
      (s) => (s.decoration as unknown as { spec: { widget: { checked?: boolean } } }).spec.widget.checked !== undefined,
    );
    expect(widgets).toHaveLength(2);
    const checkedStates = widgets.map(
      (s) => (s.decoration as unknown as { spec: { widget: { checked: boolean } } }).spec.widget.checked,
    );
    expect(checkedStates).toEqual([false, true]);
  });

  it("reveals the raw task marker on the active line", () => {
    const doc = "- [ ] open\n";
    const specs = buildForDoc(doc, [3]);
    const widgets = widgetSpecs(specs).filter(
      (s) => (s.decoration as unknown as { spec: { widget: { checked?: boolean } } }).spec.widget.checked !== undefined,
    );
    expect(widgets).toHaveLength(0);
  });

  it("hides the list bullet of a task item and the gap so only the checkbox shows", () => {
    const doc = "- [ ] open\ncursor\n";
    const state = EditorState.create({ doc, extensions: [markdown({ base: markdownLanguage })] });
    const specs = buildForDoc(doc, [state.doc.line(2).from]);
    // The ListMark "-" and the space before the checkbox (0..2) must be one
    // plain replace (hidden), not a bullet widget.
    const dashReplace = specs.find(
      (s) =>
        s.from === 0 &&
        s.to === 2 &&
        (s.decoration as unknown as { spec: { widget?: unknown; class?: string } }).spec.widget === undefined &&
        (s.decoration as unknown as { spec: { class?: string } }).spec.class === undefined,
    );
    expect(dashReplace).toBeDefined();
  });
});

describe("toggleTaskAt", () => {
  function viewFor(doc: string): EditorView {
    const state = EditorState.create({
      doc,
      extensions: [markdown({ base: markdownLanguage }), markdownTypographyPlugin],
    });
    return new EditorView({ state });
  }

  it("checks an unchecked task", () => {
    const view = viewFor("- [ ] open\n");
    expect(toggleTaskAt(view, 2)).toBe(true);
    expect(view.state.doc.toString()).toBe("- [x] open\n");
    view.destroy();
  });

  it("unchecks a checked task", () => {
    const view = viewFor("- [X] done\n");
    expect(toggleTaskAt(view, 2)).toBe(true);
    expect(view.state.doc.toString()).toBe("- [ ] done\n");
    view.destroy();
  });

  it("toggles a task in an ordered list", () => {
    const view = viewFor("1. [ ] step\n");
    expect(toggleTaskAt(view, 4)).toBe(true);
    expect(view.state.doc.toString()).toBe("1. [x] step\n");
    view.destroy();
  });

  it("toggles an indented task", () => {
    const view = viewFor("  - [ ] nested\n");
    expect(toggleTaskAt(view, 5)).toBe(true);
    expect(view.state.doc.toString()).toBe("  - [x] nested\n");
    view.destroy();
  });

  it("returns false on a non-task line", () => {
    const view = viewFor("plain text\n");
    expect(toggleTaskAt(view, 2)).toBe(false);
    expect(view.state.doc.toString()).toBe("plain text\n");
    view.destroy();
  });
});

// ─── Bullets ──────────────────────────────────────────────────────────────

describe("bullet decorations", () => {
  it("replaces a bullet list mark with a widget on inactive lines", () => {
    const doc = "- item\ncursor\n";
    const state = EditorState.create({ doc, extensions: [markdown({ base: markdownLanguage })] });
    const specs = buildForDoc(doc, [state.doc.line(2).from]);
    const bullets = widgetSpecs(specs);
    expect(bullets.some((s) => s.from === 0 && s.to === 1)).toBe(true);
  });

  it("keeps the raw bullet on the active line", () => {
    const specs = buildForDoc("- item\n", [3]);
    expect(widgetSpecs(specs)).toHaveLength(0);
  });

  it("marks ordered list numbers with a class instead of a widget", () => {
    const doc = "1. first\ncursor\n";
    const state = EditorState.create({ doc, extensions: [markdown({ base: markdownLanguage })] });
    const specs = buildForDoc(doc, [state.doc.line(2).from]);
    const numMark = specs.find(
      (s) => (s.decoration as unknown as { spec: { class?: string } }).spec?.class === "cm-md-list-num",
    );
    expect(numMark).toBeDefined();
    expect(widgetSpecs(specs)).toHaveLength(0);
  });
});

// ─── Horizontal rules ─────────────────────────────────────────────────────

describe("horizontal rule decorations", () => {
  it("replaces the rule text with a widget on inactive lines", () => {
    const doc = "above\n\n---\n\nbelow\n";
    const specs = buildForDoc(doc, [0]);
    const rules = widgetSpecs(specs).filter((s) => s.to - s.from === 3);
    expect(rules).toHaveLength(1);
  });

  it("reveals the raw rule on the active line", () => {
    const doc = "above\n\n---\n\nbelow\n";
    const rulePos = doc.indexOf("---");
    const specs = buildForDoc(doc, [rulePos + 1]);
    const rules = widgetSpecs(specs).filter((s) => s.to - s.from === 3);
    expect(rules).toHaveLength(0);
  });
});

// ─── Autolinks ────────────────────────────────────────────────────────────

describe("autolink decorations", () => {
  it("styles a bare url with the link text class", () => {
    const doc = "visit https://writ.dev today\n";
    const specs = buildForDoc(doc);
    const link = specs.find(
      (s) => (s.decoration as unknown as { spec: { class?: string } }).spec?.class === "cm-md-link-text",
    );
    expect(link).toBeDefined();
    expect(link!.from).toBe(doc.indexOf("https://"));
  });
});

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
