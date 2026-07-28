import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { spellingField, setSpellingLints, spellingEntries } from "../../editor/spelling";
import type { SpellingLint } from "../../types/spelling";

// The suggestions a user clicks travel from Rust, through a decoration spec, to
// the menu. A break anywhere in that chain shows an underlined word whose only
// offered action is "Add to dictionary".
function viewWith(doc: string, lints: SpellingLint[]) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const view = new EditorView({
    state: EditorState.create({ doc, extensions: [spellingField] }),
    parent: container,
  });
  view.dispatch({ effects: setSpellingLints.of(lints) });
  return view;
}

const OGANIC: SpellingLint = {
  fromUtf16: 12,
  toUtf16: 18,
  kind: "Spelling",
  message: "Did you mean to spell `oganic` this way?",
  suggestions: ["organic", "oceanic", "botanic"],
  confident: true,
};

describe("suggestions survive the trip into the decoration", () => {
  it("reads back every suggestion the lint carried", () => {
    const view = viewWith("I want more oganic.", [OGANIC]);
    const entries = spellingEntries(view.state);
    expect(entries).toHaveLength(1);
    expect(entries[0].word).toBe("oganic");
    expect(entries[0].suggestions).toEqual(["organic", "oceanic", "botanic"]);
  });

  it("keeps them after an unrelated edit remaps the range", () => {
    const view = viewWith("I want more oganic.", [OGANIC]);
    view.dispatch({ changes: { from: 0, to: 0, insert: "Well, " } });
    const entries = spellingEntries(view.state);
    expect(entries).toHaveLength(1);
    expect(entries[0].suggestions).toEqual(["organic", "oceanic", "botanic"]);
    // Remapped, still pointing at the word.
    expect(view.state.doc.sliceString(entries[0].from, entries[0].to)).toBe("oganic");
  });
});
