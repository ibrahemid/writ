import { describe, it, expect, vi, beforeEach } from "vitest";

// A rename that rewrote links in other files and a Move to Trash each finish
// with a toast that says what happened and carries the way back.

const h = vi.hoisted(() => ({
  showToast: vi.fn(),
  requestChoice: vi.fn(),
  requestConfirm: vi.fn(),
  countLinksTo: vi.fn(),
  renameWithLinks: vi.fn(),
  undoRename: vi.fn(),
  buffers: vi.fn(),
  undoDeleteNote: vi.fn(),
  tabsDeleteNote: vi.fn(),
  openPath: vi.fn(),
  reportOpenOutcome: vi.fn(),
}));

vi.mock("../../stores/global/notifications", () => ({ showToast: h.showToast }));
vi.mock("../../stores/global/dialogs", () => ({
  requestChoice: h.requestChoice,
  requestConfirm: h.requestConfirm,
}));
vi.mock("../../stores/global/rename-links", () => ({
  renameLinksStore: {
    countLinksTo: h.countLinksTo,
    renameWithLinks: h.renameWithLinks,
    undoRename: h.undoRename,
  },
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    buffers: h.buffers,
    deleteNote: vi.fn(),
    undoDeleteNote: h.undoDeleteNote,
  },
}));
vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({ tabs: { deleteNote: h.tabsDeleteNote, openPath: h.openPath } }),
  },
}));
vi.mock("../../stores/global/notes", () => ({ notesStore: { contains: () => true } }));
vi.mock("../../stores/global/open-report", () => ({ reportOpenOutcome: h.reportOpenOutcome }));

import { confirmAndDeleteNote, renameNoteAndLinks } from "../../lib/note-actions";
import type { ToastRequest } from "../../stores/global/notifications";

function lastToast(): ToastRequest {
  const calls = h.showToast.mock.calls;
  return calls[calls.length - 1][0] as ToastRequest;
}

beforeEach(() => {
  for (const fn of Object.values(h)) fn.mockReset();
  h.buffers.mockReturnValue([
    { id: "n-1", title: "Plan", source_path: "/notes/Plan.md" },
  ]);
});

describe("a rename that rewrote links", () => {
  it("says how many files it updated and offers Undo for that rename", async () => {
    h.countLinksTo.mockResolvedValue(3);
    h.requestChoice.mockResolvedValue("confirm");
    h.renameWithLinks.mockResolvedValue({
      renamed_path: "/notes/Plan B.md",
      updated: 3,
      updated_paths: ["/notes/a.md", "/notes/b.md", "/notes/c.md"],
      skipped: [],
      undo_token: "t-1",
    });

    await renameNoteAndLinks("n-1", "Plan B");

    const toast = lastToast();
    expect(toast.text).toBe("Updated links in 3 files");
    expect(toast.action?.label).toBe("Undo");
    await toast.action!.run();
    expect(h.undoRename).toHaveBeenCalledWith("t-1");
  });

  it("says nothing when no other file was rewritten", async () => {
    h.countLinksTo.mockResolvedValue(0);
    h.renameWithLinks.mockResolvedValue({
      renamed_path: "/notes/Plan B.md",
      updated: 0,
      updated_paths: [],
      skipped: [],
      undo_token: "t-2",
    });

    await renameNoteAndLinks("n-1", "Plan B");

    expect(h.showToast).not.toHaveBeenCalled();
  });
});

describe("Move to Trash", () => {
  it("says where the file went and puts it back on Undo", async () => {
    h.requestConfirm.mockResolvedValue(true);
    h.tabsDeleteNote.mockResolvedValue("trash-1");
    h.undoDeleteNote.mockResolvedValue("/notes/Plan.md");
    h.openPath.mockResolvedValue({ kind: "opened", doc: { id: "n-2" }, existed: false });

    await confirmAndDeleteNote("n-1");

    expect(h.requestConfirm.mock.calls[0][0]).toMatchObject({ danger: true });
    const toast = lastToast();
    expect(toast.text).toBe('Moved "Plan.md" to the Trash');
    await toast.action!.run();
    expect(h.undoDeleteNote).toHaveBeenCalledWith("trash-1");
    expect(h.openPath).toHaveBeenCalledWith("/notes/Plan.md");
    expect(h.reportOpenOutcome).toHaveBeenCalled();
  });

  it("offers no Undo when there is nothing to put back", async () => {
    h.requestConfirm.mockResolvedValue(true);
    h.tabsDeleteNote.mockResolvedValue(null);

    await confirmAndDeleteNote("n-1");

    expect(lastToast().action).toBeUndefined();
  });

  it("says so when the file cannot be put back", async () => {
    h.requestConfirm.mockResolvedValue(true);
    h.tabsDeleteNote.mockResolvedValue("trash-1");
    h.undoDeleteNote.mockRejectedValue("the name Plan.md is already taken");
    vi.spyOn(console, "error").mockImplementation(() => {});

    await confirmAndDeleteNote("n-1");
    await lastToast().action!.run();

    expect(h.showToast).toHaveBeenLastCalledWith('Could not put "Plan.md" back.', "error");
    expect(h.openPath).not.toHaveBeenCalled();
  });

  it("adds the reason Rust wrote for a person, and only that", async () => {
    h.requestConfirm.mockResolvedValue(true);
    h.tabsDeleteNote.mockResolvedValue("trash-1");
    h.undoDeleteNote.mockRejectedValue("Your folder moved since the file was deleted.");
    vi.spyOn(console, "error").mockImplementation(() => {});

    await confirmAndDeleteNote("n-1");
    await lastToast().action!.run();

    expect(h.showToast).toHaveBeenLastCalledWith(
      'Could not put "Plan.md" back. Your folder moved since the file was deleted.',
      "error",
    );

    h.undoDeleteNote.mockRejectedValue("consistency error: buffer not found: 3f2a9c1e");
    await confirmAndDeleteNote("n-1");
    await lastToast().action!.run();
    expect(h.showToast).toHaveBeenLastCalledWith('Could not put "Plan.md" back.', "error");
  });
});
