import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import { createSignal } from "solid-js";

// The notes a rename could not rewrite are named on screen, not counted: the
// person has to know which files still carry the old name to go and fix them,
// and why each one was left.

interface SkippedNote {
  name: string;
  reason: string;
}

const [notes, setNotes] = createSignal<SkippedNote[]>([]);
const cleared = vi.fn(() => setNotes([]));
vi.mock("../../stores/global/rename-links", () => ({
  renameLinksStore: {
    skippedNotes: () => notes(),
    clearSkipped: () => cleared(),
  },
}));

import RenameSkippedBar from "../../components/Editor/RenameSkippedBar";

afterEach(() => {
  cleanup();
  setNotes([]);
  vi.clearAllMocks();
});

describe("the notes a rename left alone", () => {
  it("shows nothing when every note was rewritten", () => {
    const { container } = render(() => <RenameSkippedBar />);
    expect(container.querySelector(".rename-skipped-bar")).toBeNull();
  });

  it("names each note it left unchanged, and why", () => {
    setNotes([
      { name: "Second.md", reason: "has not finished downloading." },
      { name: "Third.md", reason: "could be linking to another note of the same name." },
    ]);
    const { container } = render(() => <RenameSkippedBar />);

    expect(container.querySelector(".rename-skipped-bar-heading")?.textContent).toBe(
      "Left 2 notes unchanged:",
    );
    const lines = Array.from(
      container.querySelectorAll(".rename-skipped-bar-list li"),
    ).map((line) => line.textContent);
    expect(lines).toEqual([
      "Second.md: has not finished downloading.",
      "Third.md: could be linking to another note of the same name.",
    ]);
  });

  it("counts one note in the singular", () => {
    setNotes([{ name: "Second.md", reason: "is read-only." }]);
    const { container } = render(() => <RenameSkippedBar />);

    expect(container.querySelector(".rename-skipped-bar-heading")?.textContent).toBe(
      "Left 1 note unchanged:",
    );
  });

  it("goes away when it has been read", () => {
    setNotes([{ name: "Second.md", reason: "is read-only." }]);
    const { getByText, container } = render(() => <RenameSkippedBar />);

    fireEvent.click(getByText("Dismiss"));

    expect(cleared).toHaveBeenCalled();
    expect(container.querySelector(".rename-skipped-bar")).toBeNull();
  });
});
