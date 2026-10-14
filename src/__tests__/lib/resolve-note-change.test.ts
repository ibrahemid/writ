import { describe, it, expect, vi, beforeEach } from "vitest";
import { Text } from "@codemirror/state";

// The one path that can lose a note's text: the answer to a file that changed
// while the document held text the file does not. What is sent as "mine" is
// the document, and what comes back decides whether the tab takes the file's
// text or keeps its own.

const api = vi.hoisted(() => ({
  resolveExternalChange: vi.fn(),
  readBufferContent: vi.fn().mockResolvedValue("the file's own text\n"),
}));
const toast = vi.hoisted(() => ({ showToast: vi.fn() }));

vi.mock("../../services/tauri", () => ({
  ...api,
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
}));
vi.mock("../../stores/global/notifications", () => toast);
vi.mock("../../stores/global/dialogs", () => ({
  requestConfirm: vi.fn(),
}));
vi.mock("../../lib/log", () => ({ logFailure: vi.fn() }));

const ON_SCREEN = Text.of(["my unsaved text", ""]);

const editor = vi.hoisted(() => ({
  currentBufferId: vi.fn(() => "n-1"),
  getActiveText: vi.fn(() => ({ text: "my unsaved text\n", usedSelection: false })),
  liveDoc: vi.fn((): unknown => null),
  liveTextOf: vi.fn((): { content: string; current: boolean } | undefined => ({
    content: "my unsaved text\n",
    current: true,
  })),
  recordFileEvent: vi.fn(),
  scheduleAutosave: vi.fn(),
  applyExternalContent: vi.fn(),
  noteSaved: vi.fn(),
}));
const tabs = vi.hoisted(() => ({ openFile: vi.fn().mockResolvedValue(undefined) }));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => ({ editor, tabs }) },
}));

import { resolveNoteChange } from "../../lib/note-actions";

beforeEach(() => {
  vi.clearAllMocks();
  editor.currentBufferId.mockReturnValue("n-1");
  editor.getActiveText.mockReturnValue({ text: "my unsaved text\n", usedSelection: false });
  editor.liveTextOf.mockReturnValue({ content: "my unsaved text\n", current: true });
  editor.liveDoc.mockReturnValue(ON_SCREEN);
  api.readBufferContent.mockResolvedValue("the file's own text\n");
  tabs.openFile.mockResolvedValue(undefined);
});

describe("answering a file that changed outside Writ", () => {
  it("sends the text on screen as mine", async () => {
    api.resolveExternalChange.mockResolvedValue({
      conflict_copy_path: "/notes/note (conflict).md",
      content: null,
      disk_hash: "abc",
    });

    await resolveNoteChange("n-1", "keep_mine");

    expect(api.resolveExternalChange).toHaveBeenCalledWith(
      "n-1",
      "keep_mine",
      "my unsaved text\n",
    );
  });

  it("does nothing while the editor is holding another note", async () => {
    // The half-second a tab takes to load. The document is not there to send,
    // and the note's file is the version that changed, so reading it would
    // send the file's own text as mine and write the unsaved text nowhere.
    editor.currentBufferId.mockReturnValue("n-2");

    await resolveNoteChange("n-1", "keep_mine");

    expect(api.resolveExternalChange).not.toHaveBeenCalled();
    expect(api.readBufferContent).not.toHaveBeenCalled();
    expect(editor.recordFileEvent).not.toHaveBeenCalled();
  });

  it("does nothing when there is no document to read", async () => {
    editor.getActiveText.mockReturnValue(null as never);

    await resolveNoteChange("n-1", "use_disk");

    expect(api.resolveExternalChange).not.toHaveBeenCalled();
  });

  it("puts the file's text into the tab when the file wins", async () => {
    api.resolveExternalChange.mockResolvedValue({
      conflict_copy_path: "/notes/note (conflict).md",
      content: "the file's own text\n",
      disk_hash: "def",
    });

    await resolveNoteChange("n-1", "use_disk");

    expect(editor.applyExternalContent).toHaveBeenCalledWith("n-1", "the file's own text\n");
    expect(editor.noteSaved).not.toHaveBeenCalled();
    expect(editor.recordFileEvent).toHaveBeenCalledWith("n-1", "settled");
  });

  it("records the file it just wrote when the document wins", async () => {
    // No new text to put in the tab, but the note has to stop reading dirty
    // against the file it was just written to, or the next change asks again.
    api.resolveExternalChange.mockResolvedValue({
      conflict_copy_path: "/notes/note (conflict).md",
      content: null,
      disk_hash: "ghi",
    });

    await resolveNoteChange("n-1", "keep_mine");

    expect(editor.noteSaved).toHaveBeenCalledWith("n-1", "ghi", false, { source: ON_SCREEN, current: true });
    expect(editor.applyExternalContent).not.toHaveBeenCalled();
  });

  it("says the text it wrote is not the note's own any more when typing landed during the answer", async () => {
    api.resolveExternalChange.mockResolvedValue({
      conflict_copy_path: "/notes/note (conflict).md",
      content: null,
      disk_hash: "ghi",
    });
    editor.liveTextOf.mockReturnValue({ content: "my unsaved text\nand more\n", current: true });

    await resolveNoteChange("n-1", "keep_mine");

    expect(editor.noteSaved).toHaveBeenCalledWith("n-1", "ghi", false, { source: ON_SCREEN, current: false });
    expect(editor.scheduleAutosave).toHaveBeenCalledWith("n-1", "my unsaved text\nand more\n", 0, true);
  });

  it("says the text it wrote is not known to be the note's own when nothing says what the note holds", async () => {
    // A tab switched away from mid-answer whose hold ended before the answer
    // landed: its typing may have gone out in a write of its own.
    api.resolveExternalChange.mockResolvedValue({
      conflict_copy_path: "/notes/note (conflict).md",
      content: null,
      disk_hash: "ghi",
    });
    editor.liveTextOf.mockReturnValue(undefined);

    await resolveNoteChange("n-1", "keep_mine");

    expect(editor.noteSaved).toHaveBeenCalledWith("n-1", "ghi", false, { source: ON_SCREEN, current: false });
    expect(editor.scheduleAutosave).not.toHaveBeenCalled();
  });

  it("does not count held text the store cannot vouch for as the note's, even when it is the same text", async () => {
    api.resolveExternalChange.mockResolvedValue({
      conflict_copy_path: "/notes/note (conflict).md",
      content: null,
      disk_hash: "ghi",
    });
    editor.liveTextOf.mockReturnValue({ content: "my unsaved text\n", current: false });

    await resolveNoteChange("n-1", "keep_mine");

    expect(editor.noteSaved).toHaveBeenCalledWith("n-1", "ghi", false, { source: ON_SCREEN, current: false });
    expect(editor.scheduleAutosave).not.toHaveBeenCalled();
  });

  it("writes held text the store cannot vouch for without marking the write as the note's", async () => {
    api.resolveExternalChange.mockResolvedValue({
      conflict_copy_path: "/notes/note (conflict).md",
      content: null,
      disk_hash: "ghi",
    });
    editor.liveTextOf.mockReturnValue({ content: "an older text the hold kept\n", current: false });

    await resolveNoteChange("n-1", "keep_mine");

    expect(editor.noteSaved).toHaveBeenCalledWith("n-1", "ghi", false, { source: ON_SCREEN, current: false });
    expect(editor.scheduleAutosave).toHaveBeenCalledWith("n-1", "an older text the hold kept\n", 0, false);
  });

  it("opens the copy only for the answer that asked for both", async () => {
    api.resolveExternalChange.mockResolvedValue({
      conflict_copy_path: "/notes/note (conflict).md",
      content: "the file's own text\n",
      disk_hash: "jkl",
    });

    await resolveNoteChange("n-1", "use_disk");
    expect(tabs.openFile).not.toHaveBeenCalled();

    await resolveNoteChange("n-1", "keep_both");
    expect(tabs.openFile).toHaveBeenCalledWith("/notes/note (conflict).md");
  });

  it("opens nothing when the two texts were the same text", async () => {
    api.resolveExternalChange.mockResolvedValue({
      conflict_copy_path: null,
      content: null,
      disk_hash: "mno",
    });

    await resolveNoteChange("n-1", "keep_both");

    expect(tabs.openFile).not.toHaveBeenCalled();
    expect(editor.recordFileEvent).toHaveBeenCalledWith("n-1", "settled");
  });

  it("says why it stopped, in the words the same refusal uses elsewhere", async () => {
    api.resolveExternalChange.mockRejectedValue(
      new Error("ERR_FILE_NOT_DOWNLOADED: the file is a placeholder"),
    );

    await resolveNoteChange("n-1", "keep_mine");

    const [message, kind] = toast.showToast.mock.calls[0];
    expect(message).toContain("has not finished downloading");
    expect(message).not.toContain("ERR_");
    expect(kind).toBe("error");
  });

  it("leaves the question up when the answer did not land", async () => {
    api.resolveExternalChange.mockRejectedValue(new Error("ERR_PERMISSION_DENIED"));

    await resolveNoteChange("n-1", "use_disk");

    expect(editor.recordFileEvent).not.toHaveBeenCalled();
    expect(editor.applyExternalContent).not.toHaveBeenCalled();
  });
});
