import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import type { NoteSaveStatus } from "../../stores/global/save-status";

const fixtures = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [statuses, setStatuses] = createSignal<Record<string, NoteSaveStatus>>({});
  return { statuses, setStatuses };
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
  readDiskState: vi.fn(async () => null as { hash: string } | null),
}));
const { saveCopyOfNote, retrySave, readDiskState } = stubs;

vi.mock("../../lib/note-actions", () => ({ saveCopyOfNote: stubs.saveCopyOfNote }));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { retrySave: stubs.retrySave, readDiskState: stubs.readDiskState } }),
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
    readDiskState.mockResolvedValueOnce(null);
    const { getByText, container } = render(() => <SaveFailureBar noteId="one" />);

    fireEvent.click(getByText("Try again"));
    await vi.waitFor(() =>
      expect(container.textContent).toContain("Still downloading."),
    );

    expect(readDiskState).toHaveBeenCalledWith("one");
    expect(retrySave).not.toHaveBeenCalled();
  });

  it("writes again once the file has arrived", async () => {
    fixtures.setStatuses({
      one: failure({
        code: "ERR_FILE_NOT_DOWNLOADED",
        message: "this file has not finished downloading, so your changes were not saved yet.",
      }),
    });
    readDiskState.mockResolvedValueOnce({ hash: "abc" });
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

  it("hands the note to the save-a-copy command", () => {
    fixtures.setStatuses({ one: failure() });
    const { getByText } = render(() => <SaveFailureBar noteId="one" />);

    fireEvent.click(getByText("Save a copy…"));

    expect(saveCopyOfNote).toHaveBeenCalledWith("one");
  });
});
