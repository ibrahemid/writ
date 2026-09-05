import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";

// Answering the question about a file ends it, whichever answer it is. Over a
// real editor store and a real editor view, because two of the three answers
// put the file's text into the document on the way, and a store that only
// records would end the question without the tab ever taking the text.

const api = vi.hoisted(() => ({
  resolveExternalChange: vi.fn(),
  saveBufferContent: vi.fn().mockResolvedValue(null),
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
}));

vi.mock("../../services/tauri", () => ({
  ...api,
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
}));
vi.mock("../../components/Notifications/Toast", () => ({ showToast: vi.fn() }));
vi.mock("../../components/ConfirmDialog/ConfirmDialog", () => ({
  requestConfirm: vi.fn(),
}));
vi.mock("../../lib/log", () => ({ logFailure: vi.fn() }));

const active = vi.hoisted(() => ({
  win: null as { editor: unknown; tabs: { openFile: unknown } } | null,
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => active.win },
}));

import { createEditorStore } from "../../stores/window/editor-store";
import { resolveNoteChange } from "../../lib/note-actions";
import { resetAutosave } from "../../services/autosave";
import type { ChangeChoice } from "../../types/buffer";

const NOTE = "n1";
const MINE = "the text only the tab has\n";
const THEIRS = "what the file holds now\n";

let stores: Array<{ stopSaveListener: () => void }> = [];
let views: EditorView[] = [];

/** A note asked about, held in a live editor view, wired as the window. */
function askedAbout() {
  const store = createEditorStore();
  stores.push(store);
  const view = new EditorView({
    state: EditorState.create({ doc: MINE }),
    parent: document.body,
  });
  views.push(view);
  store.registerView(view);
  store.setCurrentBufferId(NOTE);
  store.recordFileEvent(NOTE, "modified");

  const openFile = vi.fn().mockResolvedValue(undefined);
  active.win = { editor: store, tabs: { openFile } };
  return { store, view, openFile };
}

beforeEach(() => {
  vi.clearAllMocks();
  resetAutosave();
  api.saveBufferContent.mockResolvedValue(null);
  api.noteDiskState.mockResolvedValue({ state: "no_file" });
});

afterEach(() => {
  for (const store of stores) store.stopSaveListener();
  stores = [];
  for (const view of views) view.destroy();
  views = [];
  active.win = null;
  resetAutosave();
});

describe("answering the question about a file that changed", () => {
  it("ends it when the tab keeps its own text", async () => {
    const { store, view } = askedAbout();
    api.resolveExternalChange.mockResolvedValue({
      conflict_copy_path: "/notes/note (conflict).md",
      content: null,
      disk_hash: "a-digest",
    });

    await resolveNoteChange(NOTE, "keep_mine");

    expect(api.resolveExternalChange).toHaveBeenCalledWith(
      NOTE,
      "keep_mine",
      MINE,
    );
    expect(store.noteFileState(NOTE)).toBe("present");
    expect(view.state.doc.toString()).toBe(MINE);
  });

  it("ends it when the tab takes the file's text", async () => {
    const { store, view } = askedAbout();
    api.resolveExternalChange.mockResolvedValue({
      conflict_copy_path: "/notes/note (conflict).md",
      content: THEIRS,
      disk_hash: "a-digest",
    });

    await resolveNoteChange(NOTE, "use_disk");

    expect(store.noteFileState(NOTE)).toBe("present");
    expect(view.state.doc.toString()).toBe(THEIRS);
  });

  it("ends it when both texts are opened", async () => {
    const { store, view, openFile } = askedAbout();
    api.resolveExternalChange.mockResolvedValue({
      conflict_copy_path: "/notes/note (conflict).md",
      content: THEIRS,
      disk_hash: "a-digest",
    });

    await resolveNoteChange(NOTE, "keep_both");

    expect(store.noteFileState(NOTE)).toBe("present");
    expect(view.state.doc.toString()).toBe(THEIRS);
    expect(openFile).toHaveBeenCalledWith("/notes/note (conflict).md");
  });

  it("leaves the question up when the answer could not be carried out", async () => {
    // The bar is the only way back to the file, so a failure that took it away
    // would leave the person with no way to say what happens to their text.
    const { store } = askedAbout();
    api.resolveExternalChange.mockRejectedValue(new Error("ERR_WRITE_FAILED"));

    await resolveNoteChange(NOTE, "keep_mine");

    expect(store.noteFileState(NOTE)).toBe("changed");
  });

  it("answers by the note's id, so a file that moved is answered where it is", async () => {
    // The tab followed the move and the question stayed up. Nothing here
    // names a path: the command reads the note's current one
    // (`resolve_external_change_inner` in src-tauri/src/commands/buffer.rs),
    // so the answer lands on the file at its new place.
    const { store } = askedAbout();
    store.recordFileEvent(NOTE, "moved");
    expect(store.noteFileState(NOTE)).toBe("changed");
    api.resolveExternalChange.mockResolvedValue({
      conflict_copy_path: null,
      content: THEIRS,
      disk_hash: "a-digest",
    });

    await resolveNoteChange(NOTE, "use_disk");

    const call = api.resolveExternalChange.mock.calls[0] as [
      string,
      ChangeChoice,
      string,
    ];
    expect(call[0]).toBe(NOTE);
    expect(call).toHaveLength(3);
    expect(store.noteFileState(NOTE)).toBe("present");
  });
});
