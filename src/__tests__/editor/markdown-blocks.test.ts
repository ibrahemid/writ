import { describe, it, expect, afterEach, vi } from "vitest";
import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { ensureSyntaxTree, foldEffect } from "@codemirror/language";
import { MARKDOWN_EXTENSIONS } from "../../editor/markdown-math";
import {
  blockRenderField,
  diagramFailureCaption,
  diagramKey,
  drawnBlocks,
  markdownBlockField,
  markdownBlocks,
  markdownBlocksIn,
  mathKey,
  setBlockRenders,
  setBlockWindow,
} from "../../editor/markdown-blocks";
import { folding } from "../../editor/folding";
import type { BlockRenderer } from "../../editor/block-renderer";

const NEVER: BlockRenderer = {
  renderMath: () => new Promise(() => {}),
  renderDiagram: () => new Promise(() => {}),
};

const TABLE = "| Name | **Count** | Note |\n|:--|--:|:-:|\n| `a` | 2 | $x^2$ |\n| b |  | [[Plan\\|the plan]] |";
const DIAGRAM = "```mermaid\ngraph TD\n  A --> B\n```";
const MATH = "$$\nE = mc^2\n$$";
const DOC = `# Notes\n\nIntro line.\n\n${TABLE}\n\nBetween.\n\n${MATH}\n\nMore.\n\n${DIAGRAM}\n\nEnd.\n`;

function language() {
  return markdown({ base: markdownLanguage, extensions: MARKDOWN_EXTENSIONS });
}

/** A state whose block field has scanned the whole parsed document. */
function stateWith(doc: string, cursor = doc.length, extra: unknown[] = []): EditorState {
  let state = EditorState.create({
    doc,
    selection: EditorSelection.cursor(cursor),
    extensions: [language(), markdownBlocks(NEVER), ...extra] as never,
  });
  ensureSyntaxTree(state, doc.length, 5000);
  state = state.update({ effects: setBlockWindow.of({ from: 0, to: doc.length }) }).state;
  return state;
}

function drawnTexts(state: EditorState): string[] {
  return drawnBlocks(state).map((range) => state.doc.sliceString(range.from, range.to));
}

describe("the blocks Inline draws", () => {
  it("finds a table, display math and a Mermaid diagram, each by whole lines", () => {
    const state = stateWith(DOC);
    const blocks = markdownBlocksIn(state, 0, state.doc.length);
    expect(blocks.map((b) => b.kind)).toEqual(["table", "math", "diagram"]);
    expect(blocks.map((b) => state.doc.sliceString(b.from, b.to))).toEqual([TABLE, MATH, DIAGRAM]);
    expect(blocks[1].source).toBe("E = mc^2");
    expect(blocks[2].source).toBe("graph TD\n  A --> B");
  });

  it("leaves other fences, an empty diagram and a block in a quote as source", () => {
    const doc = "```js\nlet a = 1;\n```\n\n```mermaid\n```\n\n> | a | b |\n> |---|---|\n> | 1 | 2 |\n";
    const state = stateWith(doc);
    expect(markdownBlocksIn(state, 0, doc.length)).toEqual([]);
  });

  it("reads a table's cells, alignment, empty cells and wikilinks", () => {
    const state = stateWith(DOC);
    const [table] = markdownBlocksIn(state, 0, state.doc.length);
    const model = table.table!;
    expect(model.align).toEqual(["left", "right", "center"]);
    expect(model.head).toEqual([
      [{ kind: "text", text: "Name" }],
      [{ kind: "strong", runs: [{ kind: "text", text: "Count" }] }],
      [{ kind: "text", text: "Note" }],
    ]);
    expect(model.rows[0][0]).toEqual([{ kind: "code", text: "a" }]);
    expect(model.rows[0][2]).toEqual([
      { kind: "math", tex: "x^2", display: false, key: mathKey("x^2", false) },
    ]);
    expect(model.rows[1][1]).toEqual([]);
    expect(model.rows[1][2]).toEqual([{ kind: "text", text: "the plan" }]);
    expect(model.math).toEqual([mathKey("x^2", false)]);
  });
});

describe("source on the block the caret is in", () => {
  it("draws every block while the caret is outside them", () => {
    const state = stateWith(DOC, DOC.indexOf("Intro"));
    expect(drawnTexts(state)).toEqual([TABLE, MATH, DIAGRAM]);
  });

  it("a table widget yields to its source when the caret enters it", () => {
    let state = stateWith(DOC, DOC.indexOf("Intro"));
    state = state.update({ selection: EditorSelection.cursor(DOC.indexOf("| b |") + 2) }).state;
    expect(drawnTexts(state)).toEqual([MATH, DIAGRAM]);
    // The caret on the line under it gives it back.
    state = state.update({ selection: EditorSelection.cursor(DOC.indexOf("Between")) }).state;
    expect(drawnTexts(state)).toEqual([TABLE, MATH, DIAGRAM]);
  });

  it("counts a caret at either edge of a block as inside it", () => {
    let state = stateWith(DOC, DOC.indexOf(TABLE));
    expect(drawnTexts(state)).not.toContain(TABLE);
    state = state.update({ selection: EditorSelection.cursor(DOC.indexOf(TABLE) + TABLE.length) }).state;
    expect(drawnTexts(state)).not.toContain(TABLE);
  });

  it("shows a block's source where a fold starts inside it", () => {
    let state = stateWith(DOC, DOC.indexOf("Intro"), [folding]);
    const fence = DOC.indexOf(DIAGRAM);
    const firstLineEnd = state.doc.lineAt(fence).to;
    state = state.update({
      effects: foldEffect.of({ from: firstLineEnd, to: fence + DIAGRAM.length }),
    }).state;
    expect(drawnTexts(state)).toEqual([TABLE, MATH]);
  });

  it("keys a diagram by the theme it is drawn in", () => {
    expect(diagramKey("graph TD", "light")).not.toBe(diagramKey("graph TD", "dark"));
    expect(mathKey("x", true)).not.toBe(mathKey("x", false));
  });
});

const views: EditorView[] = [];

afterEach(() => {
  while (views.length > 0) views.pop()!.destroy();
  vi.useRealTimers();
});

function mount(doc: string, renderer: BlockRenderer, cursor = 0): EditorView {
  const view = new EditorView({
    state: EditorState.create({
      doc,
      selection: EditorSelection.cursor(cursor),
      extensions: [language(), markdownBlocks(renderer)] as never,
    }),
    parent: document.body,
  });
  ensureSyntaxTree(view.state, doc.length, 5000);
  view.dispatch({ effects: setBlockWindow.of({ from: 0, to: doc.length }) });
  views.push(view);
  return view;
}

async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 40));
}

describe("drawing the blocks", () => {
  it("asks once per expression and diagram, and draws what comes back", async () => {
    const renderMath = vi.fn(async (tex: string) => `<span class="katex">${tex}</span>`);
    const renderDiagram = vi.fn(async () => `<svg class="drawn"><g></g></svg>`);
    const doc = `${MATH}\n\n${DIAGRAM}\n\nAgain $$E = mc^2$$ inline.\n\n${MATH}\n`;
    const view = mount(doc, { renderMath, renderDiagram }, doc.length);
    view.dispatch({ selection: EditorSelection.cursor(doc.indexOf("Again")) });
    await settle();
    expect(renderMath).toHaveBeenCalledTimes(1);
    expect(renderMath).toHaveBeenCalledWith("E = mc^2", true);
    expect(renderDiagram).toHaveBeenCalledTimes(1);
    expect(renderDiagram).toHaveBeenCalledWith("graph TD\n  A --> B", "light");
    const math = view.dom.querySelector(".cm-md-math-block[data-status='ready'] .katex");
    expect(math?.textContent).toBe("E = mc^2");
    expect(view.dom.querySelector(".cm-md-diagram[data-status='ready'] svg.drawn")).not.toBeNull();
  });

  it("puts nothing that runs into the editor, whatever a renderer hands back", async () => {
    const hostile =
      '<svg onload="alert(1)"><script>alert(2)</script><a href="javascript:alert(3)"><text>t</text></a></svg>' +
      '<img src="x" onerror="alert(4)">';
    const view = mount(`${MATH}\n\n$x$ inline\n\n${DIAGRAM}\n\nend\n`, {
      renderMath: async () => hostile,
      renderDiagram: async () => hostile,
    }, 0);
    view.dispatch({ selection: EditorSelection.cursor(view.state.doc.length) });
    await settle();
    expect(view.dom.querySelector(".cm-md-diagram[data-status='ready'] svg")).not.toBeNull();
    expect(view.dom.querySelector("script")).toBeNull();
    for (const element of Array.from(view.dom.querySelectorAll(".cm-md-block *"))) {
      for (const attribute of Array.from(element.attributes)) {
        expect(attribute.name.startsWith("on"), attribute.name).toBe(false);
        expect(attribute.value.includes("javascript:"), attribute.value).toBe(false);
      }
    }
  });

  it("draws a table from its text, with no renderer involved", () => {
    const view = mount(`intro\n\n${TABLE}\n`, NEVER);
    const cells = Array.from(view.dom.querySelectorAll(".cm-md-table-widget td")).map(
      (cell) => cell.textContent,
    );
    expect(cells).toEqual(["a", "2", "x^2", "b", "", "the plan"]);
    const header = view.dom.querySelector(".cm-md-table-widget th:nth-child(2)") as HTMLElement;
    expect(header.style.textAlign).toBe("right");
    expect(header.querySelector("strong")?.textContent).toBe("Count");
  });

  it("puts the caret in the cell a click lands on, which brings the source back", () => {
    const doc = `intro\n\n${TABLE}\n`;
    const view = mount(doc, NEVER);
    const cells = Array.from(view.dom.querySelectorAll(".cm-md-table-widget td"));
    const two = cells.find((cell) => cell.textContent === "2")!;
    two.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, button: 0 }));
    expect(view.state.selection.main.head).toBe(doc.indexOf("| 2 |") + 2);
    expect(view.dom.querySelector(".cm-md-table-widget")).toBeNull();
    // An empty cell takes the caret just past its pipe.
    view.dispatch({ selection: EditorSelection.cursor(0) });
    const empty = Array.from(view.dom.querySelectorAll(".cm-md-table-widget td"))[4];
    empty.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, button: 0 }));
    expect(view.state.selection.main.head).toBe(doc.indexOf("| b |") + 6);
  });

  it("shows a failed diagram's source and the line its error names", async () => {
    const failing: BlockRenderer = {
      renderMath: NEVER.renderMath,
      renderDiagram: () => Promise.reject(Object.assign(new Error("Parse error on line 2"), { line: 2 })),
    };
    const view = mount(`intro\n\n${DIAGRAM}\n`, failing);
    await settle();
    const frame = view.dom.querySelector(".cm-md-diagram[data-status='failed']");
    expect(frame?.querySelector(".cm-md-block-source")?.textContent).toBe("graph TD\n  A --> B");
    expect(frame?.querySelector(".cm-md-block-caption")?.textContent).toBe(diagramFailureCaption(2));
  });

  it("records drawings in one transaction per batch", async () => {
    const view = mount(`$a$ $b$ $c$\n\nend\n`, {
      renderMath: async (tex) => `<b>${tex}</b>`,
      renderDiagram: NEVER.renderDiagram,
    }, 12);
    const seen: number[] = [];
    const original = view.dispatch.bind(view);
    vi.spyOn(view, "dispatch").mockImplementation((...args: Parameters<EditorView["dispatch"]>) => {
      const spec = args[0] as { effects?: unknown };
      const effects = Array.isArray(spec.effects) ? spec.effects : spec.effects ? [spec.effects] : [];
      for (const effect of effects) {
        if ((effect as { is?: (t: unknown) => boolean }).is?.(setBlockRenders)) seen.push(1);
      }
      original(...args);
    });
    await settle();
    expect(seen.length).toBe(1);
    expect(view.state.field(blockRenderField).size).toBe(3);
  });

  it("holds the drawn ranges in the field", () => {
    const view = mount(`intro\n\n${TABLE}\n`, NEVER);
    expect(view.state.field(markdownBlockField).drawn).toHaveLength(1);
  });
});

describe("the Inline layer together", () => {
  it("draws the blocks beside the fences and the typography, and gives a table back on caret entry", async () => {
    const { markdownInlineExtension } = await import("../../editor/markdown-typography");
    const doc = `# Title\n\nSome **bold** and $x$ math.\n\n${TABLE}\n\n\`\`\`ts\nlet a = 1;\n\`\`\`\n\n${MATH}\n\n${DIAGRAM}\n\nEnd.\n`;
    const view = new EditorView({
      state: EditorState.create({
        doc,
        selection: EditorSelection.cursor(doc.length),
        extensions: [
          language(),
          markdownInlineExtension(
            { resolve: () => new Promise(() => {}), download: () => new Promise(() => {}) },
            {
              renderMath: async (tex) => `<span class="katex">${tex}</span>`,
              renderDiagram: async () => "<svg><g></g></svg>",
            },
          ),
        ] as never,
      }),
      parent: document.body,
    });
    views.push(view);
    ensureSyntaxTree(view.state, doc.length, 5000);
    view.dispatch({ effects: setBlockWindow.of({ from: 0, to: doc.length }) });
    await settle();
    expect(view.dom.querySelector(".cm-md-table-widget")).not.toBeNull();
    expect(view.dom.querySelector(".cm-md-math-block[data-status='ready']")).not.toBeNull();
    expect(view.dom.querySelector(".cm-md-diagram[data-status='ready'] svg")).not.toBeNull();
    expect(view.dom.querySelector(".cm-md-math-inline[data-status='ready']")?.textContent).toBe("x");

    view.dispatch({ selection: EditorSelection.cursor(doc.indexOf("| b |") + 2) });
    expect(view.dom.querySelector(".cm-md-table-widget")).toBeNull();
    const lines = Array.from(view.dom.querySelectorAll(".cm-line.cm-md-table")).map((l) => l.textContent);
    expect(lines).toContain("| b |  | [[Plan\\|the plan]] |");
  });
});
