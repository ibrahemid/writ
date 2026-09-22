import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView, Decoration } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { ensureSyntaxTree } from "@codemirror/language";
import {
  buildMarkdownDecorations,
  type DecorationSpec,
} from "../../editor/markdown-typography";
import {
  InlineImageWidget,
  imageLabel,
  imageReferenceAt,
  inlineImageField,
  inlineImages,
  setInlineImage,
  type InlineImageState,
} from "../../editor/markdown-images";
import { inlineLinkTargetAt, findLinkTargets } from "../../editor/link-layer";

// ─── Helpers ──────────────────────────────────────────────────────────────

const PARSE_TIMEOUT_MS = 30_000;

type SyntaxTree = NonNullable<ReturnType<typeof ensureSyntaxTree>>;

function stateFor(doc: string, extensions: unknown[] = []): EditorState {
  return EditorState.create({
    doc,
    extensions: [markdown({ base: markdownLanguage }), ...extensions] as never,
  });
}

function treeFor(state: EditorState): SyntaxTree {
  const tree = ensureSyntaxTree(state, state.doc.length, PARSE_TIMEOUT_MS);
  expect(tree?.length).toBe(state.doc.length);
  return tree!;
}

function buildForDoc(
  doc: string,
  cursorPositions: number[] = [],
  images: ReadonlyMap<string, InlineImageState> = new Map(),
): DecorationSpec[] {
  const state = stateFor(doc);
  const tree = treeFor(state);
  return buildMarkdownDecorations(
    (from, to, cb) => tree.iterate({ from, to, enter: cb }),
    (pos) => state.doc.lineAt(pos),
    (from, to) => state.doc.sliceString(from, to),
    new Set(cursorPositions),
    0,
    doc.length,
    images,
  );
}

function classesOf(spec: DecorationSpec): string[] {
  const cls = (spec.decoration as unknown as { spec: { class?: string } }).spec?.class;
  return cls ? cls.split(" ") : [];
}

function widgetOf(spec: DecorationSpec): unknown {
  return (spec.decoration as unknown as { spec: { widget?: unknown } }).spec?.widget;
}

function isReplace(spec: DecorationSpec): boolean {
  return (
    spec.to > spec.from &&
    classesOf(spec).length === 0 &&
    (spec.decoration as unknown as { point?: boolean }).point === true
  );
}

// ─── Links: the url is hidden, the label is the target ────────────────────

describe("inline link decorations", () => {
  const doc = "See [Writ](https://example.com) now\ncursor\n";
  const urlFrom = doc.indexOf("https://");
  const urlTo = urlFrom + "https://example.com".length;

  it("hides the url of an inline link on an inactive line", () => {
    const specs = buildForDoc(doc, [doc.indexOf("cursor")]);
    const hidden = specs.find((s) => s.from === urlFrom && s.to === urlTo);
    expect(hidden).toBeDefined();
    expect(isReplace(hidden!)).toBe(true);
    expect(specs.some((s) => classesOf(s).includes("cm-md-url-dim"))).toBe(false);
  });

  it("reveals the url of an inline link on the active line", () => {
    const specs = buildForDoc(doc, [0]);
    expect(specs.some((s) => s.from === urlFrom && s.to === urlTo && isReplace(s))).toBe(false);
    expect(specs.some((s) => classesOf(s).includes("cm-md-url-dim"))).toBe(true);
  });

  it("resolves a click on a link label to the enclosing link's url", () => {
    const state = stateFor(doc);
    treeFor(state);
    const hit = inlineLinkTargetAt(state, doc.indexOf("Writ") + 1);
    expect(hit).toEqual({
      from: doc.indexOf("Writ"),
      to: doc.indexOf("Writ") + "Writ".length,
      kind: "url",
      target: "https://example.com",
    });
  });

  it("names no target outside a link", () => {
    const state = stateFor(doc);
    treeFor(state);
    expect(inlineLinkTargetAt(state, 1)).toBeNull();
  });

  it("marks the label so a click on it lands on a link", () => {
    const state = stateFor(doc);
    treeFor(state);
    const ranges = findLinkTargets(state, 0, state.doc.length);
    const label = ranges.find((r) => r.from === doc.indexOf("Writ"));
    expect(label).toEqual(
      expect.objectContaining({ kind: "url", target: "https://example.com" }),
    );
  });

  it("resolves a workspace link label to its path", () => {
    const relative = "See [spec](docs/spec.md) now\n";
    const state = stateFor(relative);
    treeFor(state);
    expect(inlineLinkTargetAt(state, relative.indexOf("spec") + 1)).toEqual({
      from: relative.indexOf("spec"),
      to: relative.indexOf("spec") + "spec".length,
      kind: "path",
      target: "docs/spec.md",
    });
  });
});

// ─── Images: the source is hidden, the picture goes under the line ────────

describe("inline image decorations", () => {
  const doc = "Shot\n![Alt text](shots/one.png)\ncursor\n";
  const imageLine = doc.indexOf("![Alt");
  const lineEnd = imageLine + "![Alt text](shots/one.png)".length;
  const srcFrom = doc.indexOf("shots/one.png");
  const srcTo = srcFrom + "shots/one.png".length;

  it("emits an image widget under the line that authored it", () => {
    const specs = buildForDoc(doc, [doc.indexOf("cursor")]);
    const widgets = specs.filter((s) => widgetOf(s) instanceof InlineImageWidget);
    expect(widgets).toHaveLength(1);
    expect(widgets[0].from).toBe(lineEnd);
    expect(widgets[0].to).toBe(lineEnd);
    expect((widgetOf(widgets[0]) as InlineImageWidget).reference).toBe("shots/one.png");
  });

  it("hides the source of an image on an inactive line", () => {
    const specs = buildForDoc(doc, [doc.indexOf("cursor")]);
    expect(specs.some((s) => s.from === srcFrom && s.to === srcTo && isReplace(s))).toBe(true);
  });

  it("reveals the source of an image on the active line", () => {
    const specs = buildForDoc(doc, [imageLine]);
    expect(specs.some((s) => s.from === srcFrom && s.to === srcTo && isReplace(s))).toBe(false);
  });

  it("keeps the widget on the active line so the picture never flickers", () => {
    const specs = buildForDoc(doc, [imageLine]);
    expect(specs.filter((s) => widgetOf(s) instanceof InlineImageWidget)).toHaveLength(1);
  });

  it("reserves the image widget's height before the bytes arrive", () => {
    const widget = new InlineImageWidget("shots/one.png", { status: "pending" });
    const dom = widget.toDOM();
    expect(dom.classList.contains("cm-md-image")).toBe(true);
    expect(dom.dataset.status).toBe("pending");
    expect(dom.querySelector("img")).toBeNull();
  });

  it("draws the picture once the bytes arrive", () => {
    const widget = new InlineImageWidget("shots/one.png", {
      status: "ready",
      dataUrl: "data:image/png;base64,AAAA",
      label: "one.png",
    });
    const image = widget.toDOM().querySelector("img");
    expect(image?.getAttribute("src")).toBe("data:image/png;base64,AAAA");
    expect(image?.getAttribute("alt")).toBe("one.png");
  });

  it("names a refused image instead of drawing a broken box", () => {
    const widget = new InlineImageWidget("shots/one.png", {
      status: "failed",
      label: "one.png",
      reason: "too_large",
    });
    const dom = widget.toDOM();
    expect(dom.querySelector("img")).toBeNull();
    expect(dom.textContent).toContain("one.png");
    expect(dom.dataset.status).toBe("failed");
  });

  it("keeps the image widget's dom node when an unrelated update fires", () => {
    const pending = new InlineImageWidget("shots/one.png", { status: "pending" });
    expect(pending.eq(new InlineImageWidget("shots/one.png", { status: "pending" }))).toBe(true);
    expect(
      pending.eq(
        new InlineImageWidget("shots/one.png", {
          status: "ready",
          dataUrl: "data:image/png;base64,AAAA",
          label: "one.png",
        }),
      ),
    ).toBe(false);
    expect(pending.eq(new InlineImageWidget("shots/two.png", { status: "pending" }))).toBe(false);
  });

  it("reads the reference out of the tree rather than the text", () => {
    const state = stateFor(doc);
    const tree = treeFor(state);
    const node = tree.resolveInner(imageLine + 1, 1);
    expect(
      imageReferenceAt(node, (from, to) => state.doc.sliceString(from, to)),
    ).toBe("shots/one.png");
  });

  it("names an image by its file name", () => {
    expect(imageLabel("shots/one.png")).toBe("one.png");
    expect(imageLabel("one.png")).toBe("one.png");
  });
});

// ─── The resolve round trip ───────────────────────────────────────────────

describe("inlineImages", () => {
  const doc = "![Alt text](shots/one.png)\n";

  function mount(resolve: (reference: string) => Promise<{ dataUrl: string }>): EditorView {
    return new EditorView({
      state: stateFor(doc, [inlineImages({ resolve })]),
      parent: document.body,
    });
  }

  it("asks for every reference in the viewport once", async () => {
    const asked: string[] = [];
    const view = mount((reference) => {
      asked.push(reference);
      return Promise.resolve({ dataUrl: "data:image/png;base64,AAAA" });
    });
    await Promise.resolve();
    await Promise.resolve();
    view.dispatch({ changes: { from: doc.length, insert: "more\n" } });
    await Promise.resolve();
    await Promise.resolve();
    expect(asked).toEqual(["shots/one.png"]);
    view.destroy();
  });

  it("holds the resolved bytes so a redraw costs no round trip", async () => {
    const view = mount(() => Promise.resolve({ dataUrl: "data:image/png;base64,AAAA" }));
    for (let tick = 0; tick < 8; tick++) await Promise.resolve();
    expect(view.state.field(inlineImageField).get("shots/one.png")).toEqual({
      status: "ready",
      dataUrl: "data:image/png;base64,AAAA",
      label: "one.png",
    });
    view.destroy();
  });

  it("records a refusal as the state of that reference", async () => {
    const refused = Object.assign(new Error("refused"), { reason: "too_large" });
    const view = mount(() => Promise.reject(refused));
    for (let tick = 0; tick < 8; tick++) await Promise.resolve();
    expect(view.state.field(inlineImageField).get("shots/one.png")).toEqual({
      status: "failed",
      label: "one.png",
      reason: "too_large",
    });
    view.destroy();
  });

  it("reads an unknown rejection as a missing file", async () => {
    const view = mount(() => Promise.reject(new Error("boom")));
    for (let tick = 0; tick < 8; tick++) await Promise.resolve();
    expect(view.state.field(inlineImageField).get("shots/one.png")).toEqual({
      status: "failed",
      label: "one.png",
      reason: "not_found",
    });
    view.destroy();
  });

  it("keeps the states of references it was not told about", () => {
    const state = stateFor(doc, [inlineImages({ resolve: () => Promise.reject(new Error()) })]);
    const next = state.update({
      effects: setInlineImage.of({
        reference: "other.png",
        state: { status: "pending" },
      }),
    }).state;
    expect(next.field(inlineImageField).get("other.png")).toEqual({ status: "pending" });
    expect(
      next.update({ changes: { from: 0, insert: "x" } }).state.field(inlineImageField).size,
    ).toBe(1);
  });
});

// ─── The decoration set the plugin hands CodeMirror ───────────────────────

describe("the image widget inside a rendered view", () => {
  it("sits after the text of its own line", () => {
    const doc = "![Alt text](shots/one.png)\ncursor\n";
    const specs = buildForDoc(doc, [doc.indexOf("cursor")]);
    const widget = specs.find((s) => widgetOf(s) instanceof InlineImageWidget);
    expect(widget!.from).toBe(doc.indexOf("\n"));
  });

  it("is an inline widget, which is the only kind a view plugin may emit", () => {
    const doc = "![Alt text](shots/one.png)\ncursor\n";
    const specs = buildForDoc(doc, [doc.indexOf("cursor")]);
    const widget = specs.find((s) => widgetOf(s) instanceof InlineImageWidget);
    expect((widget!.decoration as unknown as { block?: boolean }).block).toBeFalsy();
    expect(widget!.decoration).toBeInstanceOf(Decoration.widget({ widget: {} as never }).constructor);
  });
});
