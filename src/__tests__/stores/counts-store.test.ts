import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { createRoot, createSignal } from "solid-js";
import { Text } from "@codemirror/state";
import { createCountsStore, RECOUNT_DELAY_MS, type CountsStore } from "../../stores/window/counts-store";
import { TextCounter } from "../../editor/document-counts";
import type { EditorStore } from "../../stores/window/editor-store";
import type { FileOpenMode } from "../../types/buffer";

function doc(text: string): Text {
  return Text.of(text.split("\n"));
}

function fakeEditor() {
  const [docVersion, setDocVersion] = createSignal(0);
  const [currentBufferId, setCurrentBufferId] = createSignal<string | null>(null);
  const [largeFileMode, setLargeFileMode] = createSignal<FileOpenMode | null>(null);
  let shown: Text | null = null;
  const editor = { docVersion, currentBufferId, largeFileMode, liveDoc: () => shown } as unknown as EditorStore;
  return {
    editor,
    show(id: string, text: Text, mode: FileOpenMode = { kind: "Normal" }) {
      shown = text;
      setLargeFileMode(mode);
      setDocVersion((version) => version + 1);
      setCurrentBufferId(id);
    },
    edit(text: Text) {
      shown = text;
      setDocVersion((version) => version + 1);
    },
  };
}

let dispose: () => void = () => {};

function mount(): { counts: CountsStore; view: ReturnType<typeof fakeEditor> } {
  return createRoot((release) => {
    dispose = release;
    const view = fakeEditor();
    return { counts: createCountsStore(view.editor), view };
  });
}

describe("counts store", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    dispose();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("counts nothing while nothing shows the counts", async () => {
    const count = vi.spyOn(TextCounter.prototype, "count");
    const { counts, view } = mount();
    view.show("a", doc("one two three"));
    await vi.runAllTimersAsync();

    expect(count).not.toHaveBeenCalled();
    expect(counts.counts()).toBeNull();
  });

  it("counts the document in the view once something shows the counts", async () => {
    const { counts, view } = mount();
    view.show("a", doc("one two three"));
    counts.watch();
    await vi.runAllTimersAsync();

    expect(counts.counts()).toEqual({ words: 3, characters: 13, exact: true });
  });

  it("counts the keystrokes inside one wait once, after the wait", async () => {
    const { counts, view } = mount();
    view.show("a", doc("one"));
    counts.watch();
    await vi.runAllTimersAsync();
    const count = vi.spyOn(TextCounter.prototype, "count");

    view.edit(doc("one t"));
    await vi.advanceTimersByTimeAsync(30);
    view.edit(doc("one tw"));
    await vi.advanceTimersByTimeAsync(30);
    view.edit(doc("one two"));
    expect(count).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(RECOUNT_DELAY_MS);

    expect(count).toHaveBeenCalledTimes(1);
    expect(counts.counts()).toEqual({ words: 2, characters: 7, exact: true });
  });

  it("recounts continuous typing at most once per wait, and counts its last change", async () => {
    const { counts, view } = mount();
    view.show("a", doc("one"));
    counts.watch();
    await vi.runAllTimersAsync();
    const count = vi.spyOn(TextCounter.prototype, "count");

    const BURST_MS = 600;
    let text = "one";
    for (let at = 0; at < BURST_MS; at += 50) {
      text += " w";
      view.edit(doc(text));
      await vi.advanceTimersByTimeAsync(50);
    }
    const duringBurst = count.mock.calls.length;
    expect(duringBurst).toBeGreaterThan(0);
    expect(duringBurst).toBeLessThanOrEqual(Math.ceil(BURST_MS / RECOUNT_DELAY_MS));

    await vi.advanceTimersByTimeAsync(RECOUNT_DELAY_MS);
    expect(counts.counts()).toEqual({ words: 13, characters: text.length, exact: true });
  });

  it("drops the last note's counts as soon as another note is in the view", async () => {
    const { counts, view } = mount();
    view.show("a", doc("one two three"));
    counts.watch();
    await vi.runAllTimersAsync();

    view.show("b", doc("four"));
    expect(counts.counts()).toBeNull();
    await vi.runAllTimersAsync();
    expect(counts.counts()).toEqual({ words: 1, characters: 4, exact: true });
  });

  it("stops counting when the last thing showing the counts lets go", async () => {
    const { counts, view } = mount();
    view.show("a", doc("one"));
    const first = counts.watch();
    const second = counts.watch();
    await vi.runAllTimersAsync();
    first();
    first();
    const count = vi.spyOn(TextCounter.prototype, "count");

    view.edit(doc("one two"));
    await vi.runAllTimersAsync();
    expect(count).toHaveBeenCalledTimes(1);

    second();
    count.mockClear();
    view.edit(doc("one two three"));
    await vi.runAllTimersAsync();
    expect(count).not.toHaveBeenCalled();
  });

  it("shows a labelled projection of a large file while it is still counted", async () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => (now += 10));
    const { counts, view } = mount();
    const text = doc(Array.from({ length: 4_000 }, () => "one two three four").join("\n"));
    view.show("a", text, { kind: "LargeFile" });
    counts.watch();

    await vi.advanceTimersByTimeAsync(0);
    const projected = counts.counts();
    expect(projected?.exact).toBe(false);
    expect(projected!.words).toBeGreaterThan(0);

    await vi.runAllTimersAsync();
    expect(counts.counts()).toEqual({ words: 16_000, characters: text.length, exact: true });
  });

  it("keeps the last exact counts of a file that is not large until the recount finishes", async () => {
    const { counts, view } = mount();
    const text = doc(Array.from({ length: 4_000 }, () => "one two three four").join("\n"));
    view.show("a", text);
    counts.watch();
    await vi.runAllTimersAsync();

    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => (now += 10));
    view.edit(doc(Array.from({ length: 4_000 }, () => "five six").join("\n")));
    await vi.advanceTimersByTimeAsync(RECOUNT_DELAY_MS);
    expect(counts.counts()).toEqual({ words: 16_000, characters: text.length, exact: true });

    await vi.runAllTimersAsync();
    expect(counts.counts()?.words).toBe(8_000);
  });
});
