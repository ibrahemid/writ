import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import type { SaveState } from "../../stores/global/save-status";

const fixtures = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [states, setStates] = createSignal<Record<string, string>>({});
  return { states, setStates };
});

vi.mock("../../stores/global/save-status", () => ({
  saveStatusStore: {
    stateOf: (id: string): SaveState => (fixtures.states()[id] ?? "clean") as SaveState,
  },
}));

import SaveMarker from "../../components/SaveMarker/SaveMarker";

afterEach(() => {
  fixtures.setStates({});
  cleanup();
});

describe("SaveMarker", () => {
  it("marks nothing while the note matches its file", () => {
    const { container } = render(() => <SaveMarker noteId="one" />);
    expect(container.querySelector(".save-marker")).toBeNull();
  });

  it("marks a note with unsaved edits, and names the state for a screen reader", () => {
    fixtures.setStates({ one: "dirty" });
    const { container } = render(() => <SaveMarker noteId="one" />);

    const mark = container.querySelector<HTMLElement>(".save-marker")!;
    expect(mark).not.toBeNull();
    expect(mark.getAttribute("aria-label")).toBe("unsaved changes");
    expect(mark.classList.contains("save-marker--failed")).toBe(false);
  });

  it("gives a failed save a different shape and a different name", () => {
    fixtures.setStates({ one: "failed" });
    const { container } = render(() => <SaveMarker noteId="one" />);

    const mark = container.querySelector<HTMLElement>(".save-marker")!;
    expect(mark.classList.contains("save-marker--failed")).toBe(true);
    expect(mark.getAttribute("aria-label")).toBe("not saved");
  });

  it("marks only the note that failed, with two open", () => {
    fixtures.setStates({ one: "failed", two: "saved" });
    const { container } = render(() => (
      <>
        <span class="tab-one">
          <SaveMarker noteId="one" />
        </span>
        <span class="tab-two">
          <SaveMarker noteId="two" />
        </span>
      </>
    ));

    expect(container.querySelector(".tab-one .save-marker")).not.toBeNull();
    expect(container.querySelector(".tab-two .save-marker")).toBeNull();
  });

  it("keeps the mark while the write is in flight", () => {
    // The text is not on disk until the write lands. A mark that blinks out
    // for the length of every autosave is exactly the transient marker S1 was
    // written to get rid of.
    fixtures.setStates({ one: "saving" });
    const { container } = render(() => <SaveMarker noteId="one" />);

    const mark = container.querySelector<HTMLElement>(".save-marker")!;
    expect(mark).not.toBeNull();
    expect(mark.getAttribute("aria-label")).toBe("unsaved changes");
    expect(mark.classList.contains("save-marker--failed")).toBe(false);
  });

  it("drops the mark once the write has landed", () => {
    fixtures.setStates({ one: "saved" });
    const { container } = render(() => <SaveMarker noteId="one" />);
    expect(container.querySelector(".save-marker")).toBeNull();
  });
});
