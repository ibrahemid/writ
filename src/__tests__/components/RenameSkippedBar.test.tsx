import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import { createSignal } from "solid-js";

// The notes a rename could not rewrite are named on screen, not counted: the
// person has to know which files still carry the old name to go and fix them.

const [names, setNames] = createSignal<string[]>([]);
const cleared = vi.fn(() => setNames([]));
vi.mock("../../stores/global/rename-links", () => ({
  renameLinksStore: {
    skippedNames: () => names(),
    clearSkipped: () => cleared(),
  },
}));

import RenameSkippedBar from "../../components/Editor/RenameSkippedBar";

afterEach(() => {
  cleanup();
  setNames([]);
  vi.clearAllMocks();
});

describe("the notes a rename left alone", () => {
  it("shows nothing when every note was rewritten", () => {
    const { container } = render(() => <RenameSkippedBar />);
    expect(container.querySelector(".rename-skipped-bar")).toBeNull();
  });

  it("names each note it left unchanged", () => {
    setNames(["Second.md", "Third.md"]);
    const { container } = render(() => <RenameSkippedBar />);

    expect(container.querySelector(".rename-skipped-bar-text")?.textContent).toBe(
      "Left 2 notes unchanged: Second.md, Third.md",
    );
  });

  it("counts one note in the singular", () => {
    setNames(["Second.md"]);
    const { container } = render(() => <RenameSkippedBar />);

    expect(container.querySelector(".rename-skipped-bar-text")?.textContent).toBe(
      "Left 1 note unchanged: Second.md",
    );
  });

  it("goes away when it has been read", () => {
    setNames(["Second.md"]);
    const { getByText, container } = render(() => <RenameSkippedBar />);

    fireEvent.click(getByText("Dismiss"));

    expect(cleared).toHaveBeenCalled();
    expect(container.querySelector(".rename-skipped-bar")).toBeNull();
  });
});
