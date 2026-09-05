import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

const fixtures = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [removed, setRemoved] = createSignal<ReadonlySet<string>>(new Set<string>());
  return { removed, setRemoved };
});

const stubs = vi.hoisted(() => ({
  saveCopyOfNote: vi.fn(async () => {}),
  noteName: vi.fn((id: string) => (id === "one" ? "Meeting notes.md" : "two.md")),
  closeTab: vi.fn(async () => {}),
}));

vi.mock("../../lib/note-actions", () => ({
  saveCopyOfNote: stubs.saveCopyOfNote,
  noteName: stubs.noteName,
}));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: { isRemovedOnDisk: (id: string) => fixtures.removed().has(id) },
    tabs: { closeTab: stubs.closeTab },
  }),
}));

import RemovedOnDiskBar from "../../components/Editor/RemovedOnDiskBar";

beforeEach(() => {
  fixtures.setRemoved(new Set<string>());
  stubs.saveCopyOfNote.mockClear();
  stubs.closeTab.mockClear();
});

afterEach(cleanup);

describe("RemovedOnDiskBar", () => {
  it("shows nothing while the note still has a file", () => {
    const { container } = render(() => <RemovedOnDiskBar noteId="one" />);
    expect(container.querySelector(".removed-on-disk-bar")).toBeNull();
  });

  it("shows nothing when there is no note in front", () => {
    fixtures.setRemoved(new Set(["one"]));
    const { container } = render(() => <RemovedOnDiskBar noteId={null} />);
    expect(container.querySelector(".removed-on-disk-bar")).toBeNull();
  });

  it("names the file and says the text is still here", () => {
    fixtures.setRemoved(new Set(["one"]));
    const { container } = render(() => <RemovedOnDiskBar noteId="one" />);

    const bar = container.querySelector<HTMLElement>(".removed-on-disk-bar")!;
    expect(bar.getAttribute("role")).toBe("alert");
    expect(bar.textContent).toContain("Meeting notes.md was deleted");
    expect(bar.textContent).toContain("Your text is still here.");
  });

  it("tells only the note whose file is gone", () => {
    fixtures.setRemoved(new Set(["one"]));
    const { container } = render(() => <RemovedOnDiskBar noteId="two" />);
    expect(container.querySelector(".removed-on-disk-bar")).toBeNull();
  });

  it("hands the note to the save-a-copy command", () => {
    fixtures.setRemoved(new Set(["one"]));
    const { getByText } = render(() => <RemovedOnDiskBar noteId="one" />);

    fireEvent.click(getByText("Save a copy…"));

    expect(stubs.saveCopyOfNote).toHaveBeenCalledWith("one");
  });

  it("closes the tab on the other way out", () => {
    fixtures.setRemoved(new Set(["one"]));
    const { getByText } = render(() => <RemovedOnDiskBar noteId="one" />);

    fireEvent.click(getByText("Close"));

    expect(stubs.closeTab).toHaveBeenCalledWith("one");
  });

  it("goes away once the file is back", () => {
    fixtures.setRemoved(new Set(["one"]));
    const { container } = render(() => <RemovedOnDiskBar noteId="one" />);
    expect(container.querySelector(".removed-on-disk-bar")).not.toBeNull();

    fixtures.setRemoved(new Set<string>());

    expect(container.querySelector(".removed-on-disk-bar")).toBeNull();
  });
});
