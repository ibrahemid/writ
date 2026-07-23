import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { SpellingLint } from "../../types/spelling";

const checkMock = vi.fn();
const addIgnoredMock = vi.fn();

vi.mock("../../services/tauri", () => ({
  checkSpelling: (text: string) => checkMock(text),
  spellingAddIgnoredWord: (word: string) => addIgnoredMock(word),
}));

import { spellingStore } from "../../stores/global/spelling";
import { setSpellingLints } from "../../editor/spelling";

interface FakeView {
  dispatch: ReturnType<typeof vi.fn>;
  state: { doc: { length: number; sliceString: () => string } };
}

function fakeView(): FakeView {
  return {
    dispatch: vi.fn(),
    state: { doc: { length: 0, sliceString: () => "" } },
  };
}

function lint(word: string): SpellingLint {
  return { fromUtf16: 0, toUtf16: word.length, kind: "Spelling", message: "", suggestions: ["x"], confident: true };
}

describe("spellingStore", () => {
  let view: FakeView;

  beforeEach(() => {
    vi.useFakeTimers();
    checkMock.mockReset();
    addIgnoredMock.mockReset();
    spellingStore.detach();
    view = fakeView();
    spellingStore.attach(view as never);
  });

  afterEach(() => {
    vi.useRealTimers();
    spellingStore.detach();
  });

  it("debounces rapid edits into a single check", async () => {
    checkMock.mockResolvedValue([]);
    spellingStore.requestCheck("a");
    spellingStore.requestCheck("ab");
    spellingStore.requestCheck("abc");
    await vi.advanceTimersByTimeAsync(400);
    expect(checkMock).toHaveBeenCalledTimes(1);
    expect(checkMock).toHaveBeenCalledWith("abc");
  });

  it("dispatches lints to the view after the debounce", async () => {
    checkMock.mockResolvedValue([lint("teh")]);
    spellingStore.requestCheck("teh");
    await vi.advanceTimersByTimeAsync(400);
    expect(view.dispatch).toHaveBeenCalledTimes(1);
    const arg = view.dispatch.mock.calls[0][0];
    expect(arg.effects.is(setSpellingLints)).toBe(true);
  });

  it("drops a stale in-flight result superseded by a newer request", async () => {
    let resolveFirst: (v: SpellingLint[]) => void = () => {};
    checkMock
      .mockReturnValueOnce(new Promise<SpellingLint[]>((r) => (resolveFirst = r)))
      .mockResolvedValueOnce([lint("newer")]);

    spellingStore.requestCheck("stale");
    await vi.advanceTimersByTimeAsync(400); // fires first check, left pending

    spellingStore.requestCheck("newer");
    await vi.advanceTimersByTimeAsync(400); // fires + resolves second check

    // Now let the first (stale) request resolve.
    resolveFirst([lint("stale")]);
    await Promise.resolve();
    await Promise.resolve();

    // Exactly one dispatch: the newer result. The stale one was dropped.
    expect(view.dispatch).toHaveBeenCalledTimes(1);
  });

  it("skips the backend and clears for empty text", () => {
    spellingStore.requestCheck("");
    expect(checkMock).not.toHaveBeenCalled();
    // Dispatches an empty result set to clear decorations.
    expect(view.dispatch).toHaveBeenCalledTimes(1);
  });
});
