import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import type { NoteSaveStatus } from "../../stores/global/save-status";

const fixtures = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [statuses, setStatuses] = createSignal<Record<string, NoteSaveStatus>>({});
  // The notes whose file is being asked about, so a test can put a note under
  // a question and take it back out while the bar is on screen.
  const [held, setHeld] = createSignal<string[]>([]);
  return { statuses, setStatuses, held, setHeld };
});

vi.mock("../../stores/global/save-status", () => ({
  saveStatusStore: {
    forNote: (id: string): NoteSaveStatus =>
      fixtures.statuses()[id] ?? { state: "clean", fileName: "one.md" },
  },
}));

const stubs = vi.hoisted(() => ({
  saveCopyOfNote: vi.fn(async () => {}),
  retrySave: vi.fn(async () => ({ ok: true, failures: [] })),
  readDiskState: vi.fn(
    async () =>
      ({ state: "undescribed" }) as
        | { state: "described"; disk: { hash: string; size: number; mtime_ms: number | null } }
        | { state: "no_file" }
        | { state: "undescribed" },
  ),
}));
const { saveCopyOfNote, retrySave, readDiskState } = stubs;

vi.mock("../../lib/note-actions", () => ({ saveCopyOfNote: stubs.saveCopyOfNote }));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: {
      retrySave: stubs.retrySave,
      readDiskState: stubs.readDiskState,
      savesAreHeld: (id: string) => fixtures.held().includes(id),
    },
  }),
}));

import SaveFailureBar from "../../components/Editor/SaveFailureBar";

function failure(overrides: Partial<NoteSaveStatus["reason"]> = {}): NoteSaveStatus {
  return {
    state: "failed",
    fileName: "Meeting notes.md",
    reason: {
      code: "ERR_PERMISSION_DENIED",
      message: "you do not have permission to change this file.",
      retryable: true,
      ...overrides,
    },
  };
}

beforeEach(() => {
  fixtures.setStatuses({});
  fixtures.setHeld([]);
  retrySave.mockClear();
  readDiskState.mockClear();
  saveCopyOfNote.mockClear();
});

afterEach(cleanup);

describe("SaveFailureBar", () => {
  it("shows nothing while the note is saving normally", () => {
    fixtures.setStatuses({ one: { state: "dirty", fileName: "one.md" } });
    const { container } = render(() => <SaveFailureBar noteId="one" />);
    expect(container.querySelector(".save-failure-bar")).toBeNull();
  });

  it("names the file and the reason, and stays put", () => {
    fixtures.setStatuses({ one: failure() });
    const { container } = render(() => <SaveFailureBar noteId="one" />);

    const bar = container.querySelector<HTMLElement>(".save-failure-bar")!;
    expect(bar.getAttribute("role")).toBe("alert");
    expect(bar.textContent).toContain("Couldn't save Meeting notes.md");
    expect(bar.textContent).toContain("you do not have permission to change this file.");
    expect(bar.textContent).not.toMatch(/os error/i);
  });

  it("shows one bar, not one per failure", () => {
    fixtures.setStatuses({ one: failure(), two: failure() });
    const { container } = render(() => <SaveFailureBar noteId="one" />);
    expect(container.querySelectorAll(".save-failure-bar")).toHaveLength(1);
  });

  it("writes the note's outstanding text again on Try again", async () => {
    fixtures.setStatuses({ one: failure() });
    const { getByText } = render(() => <SaveFailureBar noteId="one" />);

    fireEvent.click(getByText("Try again"));
    await Promise.resolve();

    expect(retrySave).toHaveBeenCalledWith("one");
  });

  it("asks the file first when it had not finished downloading, and says so when it still has not", async () => {
    fixtures.setStatuses({
      one: failure({
        code: "ERR_FILE_NOT_DOWNLOADED",
        message: "this file has not finished downloading, so your changes were not saved yet.",
      }),
    });
    readDiskState.mockResolvedValueOnce({ state: "undescribed" });
    const { getByText, container } = render(() => <SaveFailureBar noteId="one" />);

    fireEvent.click(getByText("Try again"));
    await vi.waitFor(() =>
      expect(container.textContent).toContain("Still downloading."),
    );

    expect(readDiskState).toHaveBeenCalledWith("one");
    expect(retrySave).not.toHaveBeenCalled();
  });

  it("tells only the note it asked about that its file is still downloading", async () => {
    // One bar serves every tab. Note two failed for an unrelated reason and
    // must not inherit note one's answer when the person switches to it.
    const [noteId, setNoteId] = createSignal("one");
    fixtures.setStatuses({
      one: failure({
        code: "ERR_FILE_NOT_DOWNLOADED",
        message: "this file has not finished downloading, so your changes were not saved yet.",
      }),
      two: failure(),
    });
    readDiskState.mockResolvedValueOnce({ state: "undescribed" });
    const { getByText, container } = render(() => <SaveFailureBar noteId={noteId()} />);

    fireEvent.click(getByText("Try again"));
    await vi.waitFor(() => expect(container.textContent).toContain("Still downloading."));

    setNoteId("two");
    expect(container.textContent).not.toContain("Still downloading.");

    setNoteId("one");
    expect(container.textContent).toContain("Still downloading.");
  });

  it("writes again once the file has arrived", async () => {
    fixtures.setStatuses({
      one: failure({
        code: "ERR_FILE_NOT_DOWNLOADED",
        message: "this file has not finished downloading, so your changes were not saved yet.",
      }),
    });
    readDiskState.mockResolvedValueOnce({
      state: "described",
      disk: { hash: "abc", size: 3, mtime_ms: null },
    });
    const { getByText } = render(() => <SaveFailureBar noteId="one" />);

    fireEvent.click(getByText("Try again"));
    await vi.waitFor(() => expect(retrySave).toHaveBeenCalledWith("one"));
  });

  it("offers no Try again where writing the same text is stopped the same way", () => {
    fixtures.setStatuses({
      one: failure({
        code: "ERR_FILE_CHANGED_ON_DISK",
        message: "the file changed outside Writ. A copy of your version is beside it.",
        retryable: false,
      }),
    });
    const { queryByText, getByText } = render(() => <SaveFailureBar noteId="one" />);

    expect(queryByText("Try again")).toBeNull();
    expect(getByText("Save a copy…")).not.toBeNull();
  });

  // A save on the wire when the watcher reports fails under the question, and
  // it can fail for a reason that is ordinarily worth another press. The write
  // paths are all held while the question is up, so the press would reach
  // nothing and say nothing.
  it("offers no Try again while the note's file is being asked about", () => {
    fixtures.setStatuses({ one: failure({ code: "ERR_WRITE_FAILED" }) });
    fixtures.setHeld(["one"]);
    const { queryByText, getByText } = render(() => <SaveFailureBar noteId="one" />);

    expect(queryByText("Try again")).toBeNull();
    expect(getByText("Save a copy…")).not.toBeNull();
  });

  it("offers it again once the question is answered", () => {
    fixtures.setStatuses({ one: failure({ code: "ERR_WRITE_FAILED" }) });
    fixtures.setHeld(["one"]);
    const { queryByText } = render(() => <SaveFailureBar noteId="one" />);
    expect(queryByText("Try again")).toBeNull();

    fixtures.setHeld([]);

    expect(queryByText("Try again")).not.toBeNull();
  });

  it("keeps it for a note whose file nobody is asking about", () => {
    fixtures.setStatuses({ one: failure(), two: failure() });
    fixtures.setHeld(["two"]);
    const { queryByText } = render(() => <SaveFailureBar noteId="one" />);

    expect(queryByText("Try again")).not.toBeNull();
  });

  it("hands the note to the save-a-copy command", () => {
    fixtures.setStatuses({ one: failure() });
    const { getByText } = render(() => <SaveFailureBar noteId="one" />);

    fireEvent.click(getByText("Save a copy…"));

    expect(saveCopyOfNote).toHaveBeenCalledWith("one");
  });
});
