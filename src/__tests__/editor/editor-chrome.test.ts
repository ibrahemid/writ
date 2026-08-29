import { describe, it, expect } from "vitest";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { codeChrome, codeChromeFor, isCodeBuffer } from "../../editor/code-chrome";

const DOC = "fn main() {\n  println!(\"hi\");\n}\n";

function viewFor(lang: string | null) {
  const chrome = new Compartment();
  const view = new EditorView({
    state: EditorState.create({
      doc: DOC,
      extensions: [chrome.of(codeChromeFor(lang))],
    }),
  });
  return { view, chrome };
}

function hasGutter(view: EditorView): boolean {
  return view.dom.querySelector(".cm-gutters") !== null;
}

function hasActiveLine(view: EditorView): boolean {
  return view.dom.querySelector(".cm-activeLine") !== null;
}

describe("isCodeBuffer", () => {
  it("reads markdown and an undetected buffer as prose", () => {
    expect(isCodeBuffer("markdown")).toBe(false);
    expect(isCodeBuffer(null)).toBe(false);
  });

  it("reads every other language as code", () => {
    for (const lang of ["rust", "typescript", "python", "json", "sql"]) {
      expect(isCodeBuffer(lang)).toBe(true);
    }
  });
});

describe("prose surface", () => {
  it("gives a markdown buffer no gutter and no active-line background", () => {
    const { view } = viewFor("markdown");
    expect(hasGutter(view)).toBe(false);
    expect(hasActiveLine(view)).toBe(false);
    view.destroy();
  });

  it("gives an undetected buffer no gutter and no active-line background", () => {
    const { view } = viewFor(null);
    expect(hasGutter(view)).toBe(false);
    expect(hasActiveLine(view)).toBe(false);
    view.destroy();
  });

  it("keeps the reading column so the note still starts at the prose padding", () => {
    const { view } = viewFor("markdown");
    // Nothing sits left of the content, so `.cm-content` is the scroller's
    // only child and its padding is the whole left inset.
    const scroller = view.dom.querySelector(".cm-scroller")!;
    expect(Array.from(scroller.children).map((c) => c.className)).toEqual(["cm-content"]);
    view.destroy();
  });
});

describe("code surface", () => {
  it("gives a rust buffer the gutter, the line numbers and the active line", () => {
    const { view } = viewFor("rust");
    expect(hasGutter(view)).toBe(true);
    expect(view.dom.querySelector(".cm-lineNumbers")).not.toBeNull();
    expect(hasActiveLine(view)).toBe(true);
    expect(view.dom.querySelector(".cm-activeLineGutter")).not.toBeNull();
    view.destroy();
  });
});

describe("language change", () => {
  it("adds the chrome when a prose buffer turns out to be code", () => {
    const { view, chrome } = viewFor(null);
    expect(hasGutter(view)).toBe(false);

    view.dispatch({ effects: chrome.reconfigure(codeChromeFor("rust")) });
    expect(hasGutter(view)).toBe(true);
    expect(hasActiveLine(view)).toBe(true);
    view.destroy();
  });

  it("drops the chrome when a code buffer turns into a note", () => {
    const { view, chrome } = viewFor("rust");
    expect(hasGutter(view)).toBe(true);

    view.dispatch({ effects: chrome.reconfigure(codeChromeFor("markdown")) });
    expect(hasGutter(view)).toBe(false);
    expect(hasActiveLine(view)).toBe(false);
    view.destroy();
  });
});

describe("codeChromeFor", () => {
  it("hands a code buffer the shared chrome and a prose buffer nothing", () => {
    expect(codeChromeFor("rust")).toBe(codeChrome);
    expect(codeChromeFor("markdown")).toEqual([]);
    expect(codeChromeFor(null)).toEqual([]);
  });
});
