import { describe, it, expect } from "vitest";
import { EditorSelection, EditorState, type StateCommand } from "@codemirror/state";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import {
  toggleBold,
  toggleItalic,
  toggleStrikethrough,
  toggleInlineCode,
  insertLink,
  wrapOnType,
} from "../../commands/markdown-format";

function stateWith(doc: string, anchor: number, head?: number): EditorState {
  return EditorState.create({
    doc,
    selection: { anchor, head: head ?? anchor },
    extensions: [markdown({ base: markdownLanguage })],
  });
}

function apply(state: EditorState, cmd: StateCommand): EditorState {
  let result = state;
  const handled = cmd({
    state,
    dispatch: (tr) => {
      result = tr.state;
    },
  });
  expect(handled).toBe(true);
  return result;
}

describe("toggleBold", () => {
  it("wraps a selection in ** and keeps the text selected", () => {
    const next = apply(stateWith("hello world", 0, 5), toggleBold);
    expect(next.doc.toString()).toBe("**hello** world");
    expect(next.selection.main.from).toBe(2);
    expect(next.selection.main.to).toBe(7);
  });

  it("unwraps when the selection is the inner text of a strong span", () => {
    const next = apply(stateWith("**hello** world", 2, 7), toggleBold);
    expect(next.doc.toString()).toBe("hello world");
    expect(next.selection.main.from).toBe(0);
    expect(next.selection.main.to).toBe(5);
  });

  it("unwraps when the selection includes the markers", () => {
    const next = apply(stateWith("**hello** world", 0, 9), toggleBold);
    expect(next.doc.toString()).toBe("hello world");
  });

  it("wraps the word under an empty cursor", () => {
    const next = apply(stateWith("hello world", 2), toggleBold);
    expect(next.doc.toString()).toBe("**hello** world");
    expect(next.selection.main.head).toBe(4);
  });

  it("inserts an empty pair at a word boundary and centers the cursor", () => {
    const next = apply(stateWith("go ", 3), toggleBold);
    expect(next.doc.toString()).toBe("go ****");
    expect(next.selection.main.head).toBe(5);
  });

  it("unwraps when an empty cursor sits inside a strong span", () => {
    const next = apply(stateWith("**hello** world", 4), toggleBold);
    expect(next.doc.toString()).toBe("hello world");
  });

  it("wraps every range of a multiple selection", () => {
    const state = EditorState.create({
      doc: "one two",
      selection: EditorSelection.create(
        [EditorSelection.range(0, 3), EditorSelection.range(4, 7)],
        0,
      ),
      extensions: [EditorState.allowMultipleSelections.of(true), markdown({ base: markdownLanguage })],
    });
    const next = apply(state, toggleBold);
    expect(next.doc.toString()).toBe("**one** **two**");
  });
});

describe("toggleItalic", () => {
  it("wraps a selection in single asterisks", () => {
    const next = apply(stateWith("hello world", 0, 5), toggleItalic);
    expect(next.doc.toString()).toBe("*hello* world");
  });

  it("unwraps an emphasis span from inside", () => {
    const next = apply(stateWith("*hello* world", 1, 6), toggleItalic);
    expect(next.doc.toString()).toBe("hello world");
  });

  it("does not treat the emphasis inside strong markers as bold", () => {
    const next = apply(stateWith("**hello** world", 2, 7), toggleItalic);
    expect(next.doc.toString()).toBe("***hello*** world");
  });
});

describe("toggleStrikethrough", () => {
  it("wraps a selection in ~~", () => {
    const next = apply(stateWith("done item", 0, 4), toggleStrikethrough);
    expect(next.doc.toString()).toBe("~~done~~ item");
  });

  it("unwraps a strikethrough span", () => {
    const next = apply(stateWith("~~done~~ item", 2, 6), toggleStrikethrough);
    expect(next.doc.toString()).toBe("done item");
  });
});

describe("toggleInlineCode", () => {
  it("wraps a selection in backticks", () => {
    const next = apply(stateWith("use foo here", 4, 7), toggleInlineCode);
    expect(next.doc.toString()).toBe("use `foo` here");
  });

  it("unwraps inline code including its backtick marks", () => {
    const next = apply(stateWith("use `foo` here", 5, 8), toggleInlineCode);
    expect(next.doc.toString()).toBe("use foo here");
  });
});

describe("insertLink", () => {
  it("wraps a plain selection and parks the cursor in the url slot", () => {
    const next = apply(stateWith("read this doc", 5, 9), insertLink);
    expect(next.doc.toString()).toBe("read [this]() doc");
    expect(next.selection.main.head).toBe(12);
    expect(next.selection.main.empty).toBe(true);
  });

  it("uses a url selection as the target and parks the cursor in the label", () => {
    const next = apply(stateWith("see https://a.dev now", 4, 17), insertLink);
    expect(next.doc.toString()).toBe("see [](https://a.dev) now");
    expect(next.selection.main.head).toBe(5);
  });

  it("inserts an empty link at an empty cursor with the cursor in the label", () => {
    const next = apply(stateWith("see ", 4), insertLink);
    expect(next.doc.toString()).toBe("see []()");
    expect(next.selection.main.head).toBe(5);
  });

  it("selects the url of the link under the cursor instead of nesting", () => {
    const next = apply(stateWith("a [label](https://x.dev) b", 4), insertLink);
    expect(next.doc.toString()).toBe("a [label](https://x.dev) b");
    expect(next.selection.main.from).toBe(10);
    expect(next.selection.main.to).toBe(23);
  });
});

describe("wrapOnType", () => {
  it("wraps a non-empty selection when an emphasis marker is typed", () => {
    const state = stateWith("hello world", 0, 5);
    const spec = wrapOnType(state, "*");
    expect(spec).not.toBeNull();
    const next = state.update(spec!).state;
    expect(next.doc.toString()).toBe("*hello* world");
    expect(next.selection.main.from).toBe(1);
    expect(next.selection.main.to).toBe(6);
  });

  it.each(["_", "~", "`"])("wraps with %s", (marker) => {
    const state = stateWith("hi there", 0, 2);
    const next = state.update(wrapOnType(state, marker)!).state;
    expect(next.doc.toString()).toBe(`${marker}hi${marker} there`);
  });

  it("returns null for an empty selection", () => {
    expect(wrapOnType(stateWith("hello", 2), "*")).toBeNull();
  });

  it("returns null for a non-marker character", () => {
    expect(wrapOnType(stateWith("hello", 0, 5), "x")).toBeNull();
  });

  it("wraps all ranges of a multiple selection", () => {
    const state = EditorState.create({
      doc: "one two",
      selection: EditorSelection.create(
        [EditorSelection.range(0, 3), EditorSelection.range(4, 7)],
        0,
      ),
      extensions: [EditorState.allowMultipleSelections.of(true), markdown({ base: markdownLanguage })],
    });
    const next = state.update(wrapOnType(state, "*")!).state;
    expect(next.doc.toString()).toBe("*one* *two*");
  });

  it("returns null when any range is empty", () => {
    const state = EditorState.create({
      doc: "one two",
      selection: EditorSelection.create(
        [EditorSelection.range(0, 3), EditorSelection.cursor(5)],
        0,
      ),
      extensions: [EditorState.allowMultipleSelections.of(true), markdown({ base: markdownLanguage })],
    });
    expect(wrapOnType(state, "*")).toBeNull();
  });
});
