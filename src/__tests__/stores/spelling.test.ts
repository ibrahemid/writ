import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { EditorState, type TransactionSpec } from "@codemirror/state";
import type { SpellingLint } from "../../types/spelling";

const checkMock = vi.fn();
const addIgnoredMock = vi.fn();

vi.mock("../../services/tauri", () => ({
  checkSpelling: (text: string) => checkMock(text),
  spellingAddIgnoredWord: (word: string) => addIgnoredMock(word),
}));

import { spellingStore } from "../../stores/global/spelling";
import { spellingEntries, spellingExtension } from "../../editor/spelling";

class FakeView {
  state: EditorState;
  dispatched = 0;

  constructor(doc: string) {
    this.state = EditorState.create({ doc, extensions: [spellingExtension(() => {})] });
  }

  dispatch(spec: TransactionSpec) {
    this.dispatched++;
    this.state = this.state.update(spec).state;
  }

  focus() {}

  type(at: number, text: string) {
    this.dispatch({ changes: { from: at, insert: text } });
    spellingStore.requestCheck();
  }

  flagged(): string[] {
    return spellingEntries(this.state).map((entry) => `${entry.word}@${entry.from}`);
  }

  text(): string {
    return this.state.doc.toString();
  }
}

/** A lint for `word` where it first appears in `text`, the text the checker was sent. */
function lintIn(text: string, word: string): SpellingLint {
  const at = text.indexOf(word);
  if (at < 0) throw new Error(`"${word}" is not in "${text}"`);
  return { fromUtf16: at, toUtf16: at + word.length, kind: "Spelling", message: "", suggestions: ["x"], confident: true };
}

/** Answers each check with a lint for every listed word the sent text holds. */
function flags(...words: string[]) {
  checkMock.mockImplementation(async (text: string) =>
    words.filter((word) => text.includes(word)).map((word) => lintIn(text, word)),
  );
}

const PARAGRAPHS = "teh first paragraph\n\nthe second paragraph\n\nthe third one";

let view: FakeView;

function show(doc: string): FakeView {
  view = new FakeView(doc);
  spellingStore.attach(view as never);
  return view;
}

async function settle(): Promise<void> {
  await vi.advanceTimersByTimeAsync(400);
}

describe("spellingStore", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    checkMock.mockReset();
    addIgnoredMock.mockReset();
    spellingStore.detach();
  });

  afterEach(() => {
    vi.useRealTimers();
    spellingStore.detach();
  });

  it("checks the whole document in one call when it attaches", async () => {
    flags("teh");
    show(PARAGRAPHS);
    spellingStore.requestCheck();
    await settle();

    expect(checkMock).toHaveBeenCalledTimes(1);
    expect(checkMock).toHaveBeenCalledWith(PARAGRAPHS);
    expect(view.flagged()).toEqual(["teh@0"]);
  });

  it("debounces rapid edits into a single check", async () => {
    flags();
    show(PARAGRAPHS);
    spellingStore.requestCheck();
    await settle();
    checkMock.mockClear();

    const end = PARAGRAPHS.indexOf("second paragraph") + "second paragraph".length;
    view.type(end, "a");
    view.type(end + 1, "b");
    view.type(end + 2, "c");
    await settle();

    expect(checkMock).toHaveBeenCalledTimes(1);
  });

  it("sends only the paragraph an edit touched", async () => {
    flags();
    show(PARAGRAPHS);
    spellingStore.requestCheck();
    await settle();
    checkMock.mockClear();

    view.type(PARAGRAPHS.indexOf("second paragraph") + "second paragraph".length, "s");
    await settle();

    expect(checkMock.mock.calls).toEqual([["the second paragraphs"]]);
  });

  it("puts a lint from a paragraph at its place in the document", async () => {
    flags("teh");
    show(PARAGRAPHS);
    spellingStore.requestCheck();
    await settle();

    flags("teh", "secnd");
    const at = PARAGRAPHS.indexOf("second");
    view.dispatch({ changes: { from: at, to: at + "second".length, insert: "secnd" } });
    spellingStore.requestCheck();
    await settle();

    expect(view.flagged()).toEqual(["teh@0", `secnd@${at}`]);
  });

  it("keeps the marks outside the paragraph it checked, and replaces the ones inside", async () => {
    flags("teh", "thrid");
    show("teh first paragraph\n\nthe thrid one");
    spellingStore.requestCheck();
    await settle();
    expect(view.flagged()).toEqual(["teh@0", "thrid@25"]);

    flags("teh", "thrid");
    view.dispatch({ changes: { from: 25, to: 30, insert: "third" } });
    spellingStore.requestCheck();
    await settle();

    expect(checkMock).toHaveBeenLastCalledWith("the third one");
    expect(view.flagged()).toEqual(["teh@0"]);
  });

  it("checks each paragraph one edit changed on its own", async () => {
    flags();
    show(PARAGRAPHS);
    spellingStore.requestCheck();
    await settle();
    checkMock.mockClear();

    view.dispatch({
      changes: ["first", "second", "third"].map((word) => ({
        from: PARAGRAPHS.indexOf(word),
        to: PARAGRAPHS.indexOf(word) + word.length,
        insert: word.toUpperCase(),
      })),
    });
    spellingStore.requestCheck();
    await settle();

    expect(checkMock.mock.calls).toEqual([
      ["teh FIRST paragraph"],
      ["the SECOND paragraph"],
      ["the THIRD one"],
    ]);
  });

  it("sends one stretch, from the first changed paragraph to the last, for a Replace All across many paragraphs", async () => {
    const parts = Array.from({ length: 200 }, (_, i) => `paragraph ${i} has teh word`);
    const doc = parts.join("\n\n");
    flags("teh");
    show(doc);
    spellingStore.requestCheck();
    await settle();
    checkMock.mockClear();

    const changes = [];
    for (let i = 50; i < 150; i++) {
      const at = doc.indexOf(`paragraph ${i} has `) + `paragraph ${i} has `.length;
      changes.push({ from: at, to: at + 3, insert: "the" });
    }
    view.dispatch({ changes });
    spellingStore.requestCheck();
    await settle();

    expect(checkMock.mock.calls).toEqual([[parts.slice(50, 150).join("\n\n").replaceAll("teh", "the")]]);
    expect(view.flagged()).toEqual(["teh@16"]);
  });

  it("drops a result the document moved past and sends what it covered again", async () => {
    let answerFirst: (lints: SpellingLint[]) => void = () => {};
    checkMock.mockReturnValueOnce(new Promise<SpellingLint[]>((resolve) => (answerFirst = resolve)));
    show(PARAGRAPHS);
    spellingStore.requestCheck();
    await settle();
    expect(checkMock).toHaveBeenCalledTimes(1);

    view.type(view.state.doc.length, "s");
    answerFirst([lintIn(PARAGRAPHS, "teh")]);
    await vi.advanceTimersByTimeAsync(0);
    expect(view.flagged()).toEqual([]);

    flags("teh");
    await settle();
    expect(checkMock).toHaveBeenCalledTimes(2);
    expect(checkMock).toHaveBeenLastCalledWith(`${PARAGRAPHS}s`);
    expect(view.flagged()).toEqual(["teh@0"]);
  });

  it("drops a stale in-flight result superseded by a newer request", async () => {
    let answerFirst: (lints: SpellingLint[]) => void = () => {};
    checkMock
      .mockReturnValueOnce(new Promise<SpellingLint[]>((resolve) => (answerFirst = resolve)))
      .mockResolvedValueOnce([]);
    show(PARAGRAPHS);
    spellingStore.requestCheck();
    await settle();

    spellingStore.requestCheck();
    await settle();
    const dispatchedBefore = view.dispatched;

    answerFirst([lintIn(PARAGRAPHS, "teh")]);
    await vi.advanceTimersByTimeAsync(0);

    expect(view.dispatched).toBe(dispatchedBefore);
    expect(view.flagged()).toEqual([]);
  });

  it("clears an empty document without calling the checker", async () => {
    show("");
    spellingStore.requestCheck();
    await settle();

    expect(checkMock).not.toHaveBeenCalled();
    expect(view.dispatched).toBe(1);
  });

  it("leaves the marks in place when the checker fails", async () => {
    flags("teh");
    show(PARAGRAPHS);
    spellingStore.requestCheck();
    await settle();

    checkMock.mockRejectedValue(new Error("checker unavailable"));
    view.type(view.state.doc.length, "s");
    await settle();

    expect(view.flagged()).toEqual(["teh@0"]);
  });

  it("clears the count when the feature is switched off", () => {
    spellingStore.publishCount(3);
    expect(spellingStore.count()).toBe(3);
    spellingStore.deactivate();
    expect(spellingStore.count()).toBe(0);
  });
});
