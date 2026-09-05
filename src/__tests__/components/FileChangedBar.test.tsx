import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

const fixtures = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [changed, setChanged] = createSignal<ReadonlySet<string>>(new Set<string>());
  const [loaded, setLoaded] = createSignal<string | null>("one");
  return { changed, setChanged, loaded, setLoaded };
});

const stubs = vi.hoisted(() => ({
  resolveNoteChange: vi.fn(async () => {}),
  focusEditor: vi.fn(),
}));

vi.mock("../../lib/note-actions", () => ({
  resolveNoteChange: stubs.resolveNoteChange,
}));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: {
      currentBufferId: () => fixtures.loaded(),
      isFileChangedOnDisk: (id: string) => fixtures.changed().has(id),
      focusEditor: stubs.focusEditor,
    },
  }),
}));

import FileChangedBar from "../../components/Editor/FileChangedBar";

function buttons(container: HTMLElement): HTMLButtonElement[] {
  return [...container.querySelectorAll<HTMLButtonElement>(".file-changed-bar-action")];
}

beforeEach(() => {
  fixtures.setChanged(new Set<string>());
  fixtures.setLoaded("one");
  stubs.resolveNoteChange.mockClear();
  stubs.focusEditor.mockClear();
});

afterEach(cleanup);

describe("FileChangedBar", () => {
  it("shows nothing while nothing has changed under the note", () => {
    const { container } = render(() => <FileChangedBar noteId="one" />);
    expect(container.querySelector(".file-changed-bar")).toBeNull();
  });

  it("shows nothing when there is no note in front", () => {
    fixtures.setChanged(new Set(["one"]));
    const { container } = render(() => <FileChangedBar noteId={null} />);
    expect(container.querySelector(".file-changed-bar")).toBeNull();
  });

  it("says what happened, in three answers and no jargon", () => {
    fixtures.setChanged(new Set(["one"]));
    const { container } = render(() => <FileChangedBar noteId="one" />);

    const bar = container.querySelector<HTMLElement>(".file-changed-bar")!;
    expect(bar.getAttribute("role")).toBe("alertdialog");
    expect(bar.textContent).toContain("This file changed outside Writ.");
    expect(buttons(container).map((one) => one.textContent)).toEqual([
      "Keep mine",
      "Use the file on disk",
      "Show both",
    ]);
    // The words for what is happening here are the ones a person would use
    // about their own file, not the ones a version control system uses.
    for (const word of ["conflict", "merge", "resolve"]) {
      expect(bar.textContent?.toLowerCase()).not.toContain(word);
    }
  });

  it("is labelled by the sentence it is asking about", () => {
    fixtures.setChanged(new Set(["one"]));
    const { container } = render(() => <FileChangedBar noteId="one" />);

    const bar = container.querySelector<HTMLElement>(".file-changed-bar")!;
    const label = bar.getAttribute("aria-labelledby")!;
    expect(container.ownerDocument.getElementById(label)?.textContent).toBe(
      "This file changed outside Writ.",
    );
  });

  it("takes the focus when it appears", async () => {
    const { container } = render(() => <FileChangedBar noteId="one" />);
    fixtures.setChanged(new Set(["one"]));

    await waitFor(() =>
      expect(container.ownerDocument.activeElement).toBe(buttons(container)[0]),
    );
  });

  it("hands the focus back to the editor once it is answered", async () => {
    fixtures.setChanged(new Set(["one"]));
    const { container } = render(() => <FileChangedBar noteId="one" />);

    fireEvent.click(buttons(container)[0]);

    await waitFor(() => expect(stubs.focusEditor).toHaveBeenCalledTimes(1));
  });

  it("sends the choice the button stands for", async () => {
    fixtures.setChanged(new Set(["one"]));
    const { container } = render(() => <FileChangedBar noteId="one" />);

    for (const [index, choice] of ["keep_mine", "use_disk", "keep_both"].entries()) {
      stubs.resolveNoteChange.mockClear();
      fireEvent.click(buttons(container)[index]);
      await waitFor(() =>
        expect(stubs.resolveNoteChange).toHaveBeenCalledWith("one", choice),
      );
    }
  });

  it("asks only about the note in front", () => {
    fixtures.setChanged(new Set(["one"]));
    const { container } = render(() => <FileChangedBar noteId="two" />);
    expect(container.querySelector(".file-changed-bar")).toBeNull();
  });

  it("waits for the tab it is asking about to finish loading", () => {
    // Between a tab switch and the document arriving there is no text to
    // keep, so there is nothing to ask yet and no button to press.
    fixtures.setChanged(new Set(["one"]));
    fixtures.setLoaded("two");

    const { container } = render(() => <FileChangedBar noteId="one" />);

    expect(container.querySelector(".file-changed-bar")).toBeNull();
  });
});
