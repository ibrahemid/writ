import { describe, it, expect, afterEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { javascript } from "@codemirror/lang-javascript";
import { ensureSyntaxTree, foldEffect, foldedRanges } from "@codemirror/language";
import {
  FOLD_KEYS,
  foldChromeFor,
  foldTogglesIn,
  folding,
} from "../../editor/folding";

const DOC = [
  "# One",
  "",
  "First body.",
  "",
  "```sh",
  "# a comment, not a heading",
  "```",
  "",
  "## Two",
  "",
  "Second body.",
  "",
  "# Three",
  "",
  "Third body.",
].join("\n");

function markdownState(doc = DOC): EditorState {
  const state = EditorState.create({
    doc,
    extensions: [markdown({ base: markdownLanguage }), folding],
  });
  ensureSyntaxTree(state, state.doc.length, 5000);
  return state;
}

const views: EditorView[] = [];

function mount(state: EditorState, extra: unknown[] = []): EditorView {
  const view = new EditorView({
    state: EditorState.create({
      doc: state.doc,
      extensions: [markdown({ base: markdownLanguage }), folding, ...extra] as never,
    }),
    parent: document.body,
  });
  ensureSyntaxTree(view.state, view.state.doc.length, 5000);
  views.push(view);
  return view;
}

afterEach(() => {
  while (views.length > 0) views.pop()!.destroy();
});

describe("heading fold controls", () => {
  it("offers one per heading, folding its section, and none for a # line in a fence", () => {
    const state = markdownState();
    const toggles = foldTogglesIn(state, 0, state.doc.length);
    expect(toggles.map((t) => state.doc.lineAt(t.line).text)).toEqual(["# One", "## Two", "# Three"]);
    const one = toggles[0];
    // The section of "# One" runs through "## Two" to the line before "# Three".
    expect(one.from).toBe(state.doc.line(1).to);
    expect(state.doc.sliceString(one.from, one.to)).toContain("Second body.");
    expect(state.doc.sliceString(one.from, one.to)).not.toContain("Third body.");
    expect(toggles.every((t) => !t.folded)).toBe(true);
  });

  it("reports a folded section as folded, with the range it hides", () => {
    let state = markdownState();
    const [one] = foldTogglesIn(state, 0, state.doc.length);
    state = state.update({ effects: foldEffect.of({ from: one.from, to: one.to }) }).state;
    const after = foldTogglesIn(state, 0, state.doc.length);
    expect(after[0]).toMatchObject({ folded: true, from: one.from, to: one.to });
  });

  it("folds and unfolds a section from the control in the margin", () => {
    const view = mount(markdownState(), [foldChromeFor("markdown")]);
    const control = view.dom.querySelector(".cm-md-fold-control");
    expect(control).toBeInstanceOf(HTMLElement);
    control!.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, button: 0 }));
    expect(foldedRanges(view.state).size).toBe(1);
    const folded = view.dom.querySelector(".cm-md-fold[data-folded] .cm-md-fold-control");
    expect(folded).toBeInstanceOf(HTMLElement);
    folded!.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, button: 0 }));
    expect(foldedRanges(view.state).size).toBe(0);
  });
});

describe("fold chrome per kind of file", () => {
  it("gives a source file a fold gutter and prose none", () => {
    const code = new EditorView({
      state: EditorState.create({
        doc: "function f() {\n  return 1;\n}\n",
        extensions: [javascript(), folding, foldChromeFor("code")],
      }),
      parent: document.body,
    });
    views.push(code);
    expect(code.dom.querySelector(".cm-foldGutter")).toBeInstanceOf(HTMLElement);
    const prose = mount(markdownState(), [foldChromeFor("markdown")]);
    expect(prose.dom.querySelector(".cm-foldGutter")).toBeNull();
    expect(foldChromeFor("none")).toEqual([]);
  });
});

describe("fold keys", () => {
  it("fold at the cursor without Control-Alt, which is AltGr on Windows", () => {
    expect(FOLD_KEYS.map((k) => [k.key, k.mac])).toEqual([
      ["Ctrl-Shift-[", "Cmd-Alt-["],
      ["Ctrl-Shift-]", "Cmd-Alt-]"],
    ]);
  });
});
