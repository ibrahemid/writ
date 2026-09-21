import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

import type { NoteVersion } from "../../services/tauri";

// What the panel shows while it is still reading, and what the list is to a
// keyboard. The IPC layer is replaced and held open on purpose: the defect
// these cover only exists in the window between the click and the read.

const h = vi.hoisted(() => ({
  noteVersions: vi.fn(),
  noteVersionContent: vi.fn(),
  restoreNoteVersion: vi.fn(),
  copyNoteVersion: vi.fn(),
  focusEditor: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  noteVersions: h.noteVersions,
  noteVersionContent: h.noteVersionContent,
  restoreNoteVersion: h.restoreNoteVersion,
  copyNoteVersion: h.copyNoteVersion,
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: h.focusEditor } }),
}));

import NoteHistoryPanel, {
  openNoteVersions,
  closeNoteVersions,
} from "../../components/NoteHistory/NoteHistoryPanel";

const NOTE = "/notes/Launch.md";

function at(daysAgo: number, hour: number, minute: number): number {
  const stamp = new Date();
  stamp.setDate(stamp.getDate() - daysAgo);
  stamp.setHours(hour, minute, 0, 0);
  return stamp.getTime();
}

function version(id: number, atMs: number, bytes = 1_200): NoteVersion {
  return { id, at_ms: atMs, bytes };
}

function held<T>(): { promise: Promise<T>; settle: (value: T) => void } {
  let settle!: (value: T) => void;
  const promise = new Promise<T>((resolve) => {
    settle = resolve;
  });
  return { promise, settle };
}

afterEach(() => {
  closeNoteVersions();
  h.noteVersions.mockReset();
  h.noteVersionContent.mockReset();
  h.restoreNoteVersion.mockReset();
  h.copyNoteVersion.mockReset();
  h.focusEditor.mockReset();
  cleanup();
});

describe("NoteHistoryPanel while it reads", () => {
  it("does not say the note has nothing kept while the list is still coming", async () => {
    const list = held<NoteVersion[]>();
    h.noteVersions.mockReturnValue(list.promise);
    h.noteVersionContent.mockResolvedValue("what the note said\n");

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);

    await Promise.resolve();
    expect(screen.queryByText("No versions of this file yet.")).toBeNull();
    expect(screen.getByText("Loading versions…")).toBeTruthy();

    list.settle([version(4, at(0, 14, 32))]);
    await waitFor(() => expect(screen.container.querySelector(".note-versions-row")).toBeTruthy());
    expect(screen.queryByText("No versions of this file yet.")).toBeNull();
    expect(screen.queryByText("Loading versions…")).toBeNull();
  });

  it("keeps the empty state for a note that really has nothing kept", async () => {
    h.noteVersions.mockResolvedValue([]);

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);

    await waitFor(() => expect(screen.getByText("No versions of this file yet.")).toBeTruthy());
  });

  it("does not label one version's text with another version's row", async () => {
    h.noteVersions.mockResolvedValue([version(11, at(0, 14, 32)), version(12, at(2, 9, 11))]);
    h.noteVersionContent.mockResolvedValueOnce("the newest text\n");

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);
    await waitFor(() => expect(screen.getByText("the newest text")).toBeTruthy());

    const older = held<string>();
    h.noteVersionContent.mockReturnValueOnce(older.promise);
    fireEvent.click(screen.container.querySelectorAll(".note-versions-row")[1]);

    await waitFor(() => expect(h.noteVersionContent).toHaveBeenCalledWith(12));
    expect(screen.container.querySelector(".note-versions-text")?.textContent).not.toContain(
      "the newest text",
    );

    older.settle("the older text\n");
    await waitFor(() =>
      // Exact, not normalised: the pane is a <pre>, so a stray leading newline
      // from the JSX would be a blank first line on every version.
      expect(screen.container.querySelector(".note-versions-text")?.textContent).toBe(
        "the older text\n",
      ),
    );
  });

  // Two reads open at once is the ordinary keyboard path: every ArrowDown
  // starts one. An earlier read settling must not un-blank the pane while the
  // read the selected row is waiting for is still open.
  it("never shows a version the row beside it does not name, whatever settles first", async () => {
    h.noteVersions.mockResolvedValue([
      version(11, at(0, 14, 32)),
      version(12, at(2, 9, 11)),
      version(13, at(5, 9, 11)),
    ]);
    h.noteVersionContent.mockResolvedValueOnce("newest text\n");

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);
    await waitFor(() => expect(screen.getByText("newest text")).toBeTruthy());

    const middle = held<string>();
    const oldest = held<string>();
    h.noteVersionContent.mockReturnValueOnce(middle.promise);
    h.noteVersionContent.mockReturnValueOnce(oldest.promise);

    const list = screen.container.querySelector('[role="listbox"]')!;
    fireEvent.keyDown(list, { key: "ArrowDown" });
    await waitFor(() => expect(h.noteVersionContent).toHaveBeenCalledWith(12));
    fireEvent.keyDown(list, { key: "ArrowDown" });
    await waitFor(() => expect(h.noteVersionContent).toHaveBeenCalledWith(13));

    const pane = () => screen.container.querySelector(".note-versions-text")!.textContent;
    const named = () =>
      screen.container
        .querySelector('[role="option"][aria-selected="true"]')!
        .getAttribute("data-version");

    // The read the user moved past settles first.
    middle.settle("middle text\n");
    for (let i = 0; i < 4; i += 1) await Promise.resolve();

    expect(named()).toBe("13");
    expect(pane()).not.toContain("newest text");
    expect(pane()).not.toContain("middle text");

    oldest.settle("oldest text\n");
    await waitFor(() => expect(pane()).toBe("oldest text\n"));
    expect(named()).toBe("13");
  });

  it("says once that a restore keeps what the note holds now", async () => {
    h.noteVersions.mockResolvedValue([version(11, at(0, 14, 32)), version(12, at(2, 9, 11))]);
    h.noteVersionContent.mockResolvedValue("what the note said\n");

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);

    await waitFor(() =>
      expect(screen.getAllByText("Restoring keeps the current text as another version.")).toHaveLength(1),
    );
  });
});

describe("the version list as a keyboard sees it", () => {
  it("is a listbox whose rows carry the selection", async () => {
    h.noteVersions.mockResolvedValue([version(11, at(0, 14, 32)), version(12, at(2, 9, 11))]);
    h.noteVersionContent.mockResolvedValue("what the note said\n");

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);
    await waitFor(() => expect(screen.container.querySelector('[role="listbox"]')).toBeTruthy());

    const options = screen.container.querySelectorAll<HTMLButtonElement>('[role="option"]');
    expect(options).toHaveLength(2);
    expect(options[0].getAttribute("aria-selected")).toBe("true");
    expect(options[0].tabIndex).toBe(0);
    expect(options[1].tabIndex).toBe(-1);
  });

  it("moves the selection on the arrow keys and the ends", async () => {
    h.noteVersions.mockResolvedValue([
      version(11, at(0, 14, 32)),
      version(12, at(2, 9, 11)),
      version(13, at(5, 9, 11)),
    ]);
    h.noteVersionContent.mockResolvedValue("what the note said\n");

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);
    await waitFor(() => expect(screen.container.querySelector('[role="listbox"]')).toBeTruthy());

    const list = screen.container.querySelector('[role="listbox"]')!;
    fireEvent.keyDown(list, { key: "ArrowDown" });
    await waitFor(() => expect(h.noteVersionContent).toHaveBeenCalledWith(12));

    fireEvent.keyDown(list, { key: "End" });
    await waitFor(() => expect(h.noteVersionContent).toHaveBeenCalledWith(13));

    fireEvent.keyDown(list, { key: "Home" });
    await waitFor(() =>
      expect(
        screen.container.querySelectorAll<HTMLButtonElement>('[role="option"]')[0].getAttribute(
          "aria-selected",
        ),
      ).toBe("true"),
    );
  });
});

describe("closing the panel", () => {
  it("puts focus back in the editor when what opened it is gone", async () => {
    h.noteVersions.mockResolvedValue([version(11, at(0, 14, 32))]);
    h.noteVersionContent.mockResolvedValue("what the note said\n");

    const opener = document.createElement("button");
    document.body.appendChild(opener);
    opener.focus();

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);
    await waitFor(() => expect(screen.container.querySelector(".note-versions-modal")).toBeTruthy());

    opener.remove();
    closeNoteVersions();

    await waitFor(() => expect(h.focusEditor).toHaveBeenCalled());
  });
});
