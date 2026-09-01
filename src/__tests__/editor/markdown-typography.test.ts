import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { ensureSyntaxTree, syntaxHighlighting } from "@codemirror/language";
import { writHighlight } from "../../components/Editor/cm-theme";
import {
  buildMarkdownDecorations,
  markdownTypographyPlugin,
  toggleTaskAt,
  handleTaskMousedown,
  type DecorationSpec,
} from "../../editor/markdown-typography";

// ─── Helpers ──────────────────────────────────────────────────────────────

// A fresh EditorState parses only within a 20 ms budget (Work.Apply in
// @codemirror/language) and keeps whatever tree it reached when the budget
// ran out. On a loaded machine that truncates the parse partway through the
// document, so the decorations under test would cover the first line or two
// only. Force the parse to the end of the doc, and prove it got there.
const PARSE_TIMEOUT_MS = 30_000;

type SyntaxTree = NonNullable<ReturnType<typeof ensureSyntaxTree>>;

function treeFor(state: EditorState): SyntaxTree {
  const tree = ensureSyntaxTree(state, state.doc.length, PARSE_TIMEOUT_MS);
  expect(tree?.length).toBe(state.doc.length);
  return tree!;
}

function buildForDoc(
  doc: string,
  cursorPositions: number[] = [],
): DecorationSpec[] {
  const state = EditorState.create({ doc, extensions: [markdown({ base: markdownLanguage })] });
  const tree = treeFor(state);
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

function classesOf(spec: DecorationSpec): string[] {
  const cls = (spec.decoration as unknown as { spec: { class?: string } }).spec?.class;
  return cls ? cls.split(" ") : [];
}

// The decoration specs alone proved too little: a mark whose range is right
// still loses its ink when the highlight style's own span nests inside it. The
// DOM helper renders the same extensions the editor ships so the tests can see
// which span wraps which.
function renderDoc(doc: string, cursor: number): EditorView {
  const view = new EditorView({
    state: EditorState.create({
      doc,
      extensions: [
        markdown({ base: markdownLanguage }),
        syntaxHighlighting(writHighlight),
        markdownTypographyPlugin,
      ],
      selection: { anchor: cursor },
    }),
    parent: document.body,
  });
  return view;
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
      (s) =>
        s.from === 0 &&
        s.to === 0 &&
        (s.decoration as unknown as { spec: { class: string } }).spec.class.split(" ").includes("cm-line-md-h1"),
    );
    expect(lineSpec).toBeDefined();
  });

  it("emits correct classes for h2-h6", () => {
    const doc = "## H2\n### H3\n#### H4\n##### H5\n###### H6\n";
    const specs = buildForDoc(doc);
    const classes = specs
      .filter((s) => s.from === s.to)
      .flatMap((s) => (s.decoration as unknown as { spec: { class: string } }).spec.class.split(" "));
    expect(classes).toContain("cm-line-md-h2");
    expect(classes).toContain("cm-line-md-h3");
    expect(classes).toContain("cm-line-md-h4");
    expect(classes).toContain("cm-line-md-h5");
    expect(classes).toContain("cm-line-md-h6");
  });

  it("emits every heading class when the initial parse budget runs out", () => {
    // Guards the flake this helper exists for: with the clock jumped past the
    // 20 ms init budget, EditorState.create keeps only the tree it had reached,
    // and the decorations covered the first heading or two. The stall is lifted
    // before the tree is read, because treeFor's own timeout reads the clock.
    const doc = "## H2\n### H3\n#### H4\n##### H5\n###### H6\n";
    const realNow = Date.now.bind(Date);
    let slices = 0;
    Date.now = () => (slices++ > 1 ? realNow() + 10_000 : realNow());
    let state: EditorState;
    try {
      state = EditorState.create({ doc, extensions: [markdown({ base: markdownLanguage })] });
    } finally {
      Date.now = realNow;
    }

    const tree = treeFor(state);
    const specs = buildMarkdownDecorations(
      (from, to, cb) => tree.iterate({ from, to, enter: cb }),
      (pos) => state.doc.lineAt(pos),
      (from, to) => state.doc.sliceString(from, to),
      new ReadonlySet([]),
      0,
      doc.length,
    );
    const classes = specs
      .filter((s) => s.from === s.to)
      .flatMap((s) => (s.decoration as unknown as { spec: { class: string } }).spec.class.split(" "));
    for (const level of ["h2", "h3", "h4", "h5", "h6"]) {
      expect(classes).toContain(`cm-line-md-${level}`);
    }
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
  it("decorates heading markers rather than replacing them, on any line", () => {
    // ADR-030 decision 3 narrows ADR-014: a '#' says what the block is, so it
    // stays readable and hangs in the margin instead of vanishing.
    const doc = "# Hello\nsome text\n";
    const state = EditorState.create({ doc, extensions: [markdown()] });
    const cursorOnLine2 = state.doc.line(2).from;
    for (const cursors of [[cursorOnLine2], [2]]) {
      const specs = buildForDoc(doc, cursors);
      const markerSpecs = specs.filter((s) => s.from === 0 && s.to === 1);
      const classes = markerSpecs.map(
        (s) => (s.decoration as unknown as { spec: { class?: string } }).spec.class,
      );
      expect(classes).toContain("cm-md-marker-hung");
      const replaced = markerSpecs.filter(
        (s) => (s.decoration as unknown as { spec: { class?: string } }).spec.class === undefined,
      );
      expect(replaced).toHaveLength(0);
    }
  });

  it("still replaces emphasis markers on inactive lines", () => {
    const doc = "**bold** text\nsecond line\n";
    const state = EditorState.create({ doc, extensions: [markdown()] });
    const cursorOnLine2 = state.doc.line(2).from;
    const specs = buildForDoc(doc, [cursorOnLine2]);
    const replaces = specs.filter(
      (s) =>
        s.from !== s.to &&
        (s.decoration as unknown as { spec: { class?: string; widget?: unknown } }).spec.class ===
          undefined &&
        (s.decoration as unknown as { spec: { widget?: unknown } }).spec.widget === undefined,
    );
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
    const tree = treeFor(state);
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
    const tree = treeFor(state);
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
  it("starts the cm-md-blockquote mark after the quote marker", () => {
    const doc = "> quoted text\n";
    const specs = buildForDoc(doc);
    const bq = specs.find(
      (s) => (s.decoration as unknown as { spec: { class: string } }).spec?.class === "cm-md-blockquote",
    );
    expect(bq).toBeDefined();
    // The rail is this mark's left border: including the '>' would drag it into
    // the hang margin with the marker.
    expect(bq!.from).toBe(1);
    expect(bq!.to).toBe(doc.indexOf("\n"));
  });

  it("hangs the quote marker on every quoted line", () => {
    const doc = "> first\n> second\n";
    const specs = buildForDoc(doc);
    const hangs = specs.filter(
      (s) =>
        s.from === s.to &&
        classesOf(s).includes("cm-line-md-hang"),
    );
    expect(hangs.map((s) => s.from)).toEqual([0, doc.indexOf("> second")]);

    const markers = specs.filter((s) => classesOf(s).includes("cm-md-marker-hung"));
    expect(markers.map((s) => s.from)).toEqual([0, doc.indexOf("> second")]);
  });

  it("marks each quoted line separately", () => {
    const doc = "> first\n> second\n";
    const specs = buildForDoc(doc);
    const quotes = specs.filter((s) => classesOf(s).includes("cm-md-blockquote"));
    expect(quotes.map((s) => s.from)).toEqual([1, doc.indexOf("> second") + 1]);
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

  it("ignores non-primary mouse buttons", () => {
    const view = viewFor("- [ ] open\n");
    const box = document.createElement("input");
    box.type = "checkbox";
    box.className = "cm-md-task-checkbox";
    const event = new MouseEvent("mousedown", { button: 2 });
    Object.defineProperty(event, "target", { value: box });
    expect(handleTaskMousedown(event, view)).toBe(false);
    expect(view.state.doc.toString()).toBe("- [ ] open\n");
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

// ─── Inline links ─────────────────────────────────────────────────────────

describe("inline link decorations", () => {
  const doc = "See [Writ](https://example.com) now\ncursor\n";

  it("dims the url and styles only the label on an inactive line", () => {
    const specs = buildForDoc(doc, [doc.indexOf("cursor")]);
    const label = specs.find((s) => classesOf(s).includes("cm-md-link-text"));
    const url = specs.find((s) => classesOf(s).includes("cm-md-url-dim"));
    expect(label).toEqual(
      expect.objectContaining({ from: doc.indexOf("Writ"), to: doc.indexOf("Writ") + 4 }),
    );
    expect(url).toEqual(
      expect.objectContaining({
        from: doc.indexOf("https://"),
        to: doc.indexOf("https://") + "https://example.com".length,
      }),
    );
  });

  it("stops dimming the url on the active line", () => {
    const specs = buildForDoc(doc, [0]);
    expect(specs.some((s) => classesOf(s).includes("cm-md-url-dim"))).toBe(false);
  });

  it("wraps the highlighted url token so the dim ink wins the cascade", () => {
    // The grammar tags every Link descendant, url included, with tags.link, and
    // the theme paints that accent and underlined. The dim mark only shows if
    // its span is the outer one.
    const view = renderDoc(doc, doc.indexOf("cursor"));
    const dim = view.contentDOM.querySelector(".cm-md-url-dim");
    expect(dim?.textContent).toBe("https://example.com");
    expect(dim!.querySelector("span")?.textContent).toBe("https://example.com");
    expect(view.contentDOM.querySelector(".cm-md-link-text")?.textContent).toBe("Writ");
    view.destroy();
  });

  it("wraps the highlighted quote marker so the formatting ink wins", () => {
    const quoted = "> quoted line\ncursor\n";
    const view = renderDoc(quoted, quoted.indexOf("cursor"));
    const marker = view.contentDOM.querySelector(".cm-md-marker-hung");
    expect(marker?.textContent).toBe(">");
    expect(marker!.querySelector("span")?.textContent).toBe(">");
    expect(view.contentDOM.querySelector(".cm-line")?.classList.contains("cm-line-md-hang")).toBe(true);
    view.destroy();
  });
});

// ─── Fenced code ──────────────────────────────────────────────────────────

describe("fenced code decorations", () => {
  const doc = "```sh\necho hi\nmore\n```\ncursor\n";

  it("rounds the first and last line of the block only", () => {
    const specs = buildForDoc(doc, [doc.indexOf("cursor")]);
    const lines = specs.filter((s) => classesOf(s).includes("cm-md-codeblock"));
    expect(lines).toHaveLength(4);
    expect(lines.map((s) => classesOf(s).includes("cm-md-codeblock-first"))).toEqual([
      true, false, false, false,
    ]);
    expect(lines.map((s) => classesOf(s).includes("cm-md-codeblock-last"))).toEqual([
      false, false, false, true,
    ]);
  });

  it("dims the fences and the info string instead of removing them", () => {
    const specs = buildForDoc(doc, [doc.indexOf("cursor")]);
    const dimmed = specs.filter((s) => classesOf(s).includes("cm-md-marker-dim"));
    expect(dimmed.map((s) => s.from)).toEqual([0, doc.indexOf("```\n")]);
    expect(widgetSpecs(specs).some((s) => s.from === 0)).toBe(false);

    const info = specs.find((s) => classesOf(s).includes("cm-md-code-info"));
    expect(info).toEqual(expect.objectContaining({ from: 3, to: 5 }));
  });

  it("leaves the fence plain on the active line", () => {
    const specs = buildForDoc(doc, [1]);
    const dimmed = specs.filter((s) => classesOf(s).includes("cm-md-marker-dim"));
    expect(dimmed.map((s) => s.from)).toEqual([doc.indexOf("```\n")]);
    expect(specs.some((s) => classesOf(s).includes("cm-md-code-info"))).toBe(false);
  });

  it("still replaces the backticks of inline code", () => {
    const inline = "a `code` b\ncursor\n";
    const specs = buildForDoc(inline, [inline.indexOf("cursor")]);
    expect(specs.some((s) => classesOf(s).includes("cm-md-marker-dim"))).toBe(false);
    const replaced = specs.filter(
      (s) =>
        classesOf(s).length === 0 &&
        (s.decoration as unknown as { spec: { widget?: unknown } }).spec.widget === undefined,
    );
    expect(replaced.map((s) => [s.from, s.to])).toEqual([
      [2, 3],
      [7, 8],
    ]);
  });
});

// ─── Task item state ──────────────────────────────────────────────────────

describe("task item state", () => {
  const doc = "- [x] done\n- [ ] open\ncursor\n";

  it("strikes the text of a checked item and leaves an open one alone", () => {
    const specs = buildForDoc(doc, [doc.indexOf("cursor")]);
    const struck = specs.filter((s) => classesOf(s).includes("cm-md-task-done"));
    expect(struck).toHaveLength(1);
    expect(struck[0]).toEqual(
      expect.objectContaining({ from: doc.indexOf("done"), to: doc.indexOf("done") + 4 }),
    );
  });

  it("draws the box rather than the platform checkbox", () => {
    const view = renderDoc(doc, doc.indexOf("cursor"));
    const boxes = view.contentDOM.querySelectorAll(".cm-md-task-box");
    expect(boxes).toHaveLength(2);
    expect(boxes[0].hasAttribute("data-checked")).toBe(true);
    expect(boxes[1].hasAttribute("data-checked")).toBe(false);
    expect(boxes[0].querySelector("svg.cm-md-task-check")).not.toBeNull();
    expect(boxes[1].querySelector("svg.cm-md-task-check")).toBeNull();
    // The real control stays, so the click handler and a11y are unchanged.
    const input = boxes[0].querySelector("input.cm-md-task-checkbox") as HTMLInputElement;
    expect(input.checked).toBe(true);
    expect(input.getAttribute("aria-label")).toBe("Completed task");
    view.destroy();
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
