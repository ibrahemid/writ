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
  noteDiskState: vi.fn(async (): Promise<unknown> => ({ state: "no_file" })),
  recordUnsavedNotes: vi.fn().mockResolvedValue(undefined),
  discardUnsavedText: vi.fn().mockResolvedValue(undefined),
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

import { createEditorStore, DIRTY_COMPARE_IDLE_MS } from "../../stores/window/editor-store";
import { resolveNoteChange } from "../../lib/note-actions";
import { flushAutosave, resetAutosave } from "../../services/autosave";
import type { ChangeChoice, ResolveOutcome } from "../../types/buffer";

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
  api.saveBufferContent.mockReset();
  api.saveBufferContent.mockResolvedValue(null);
  api.noteDiskState.mockReset();
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

describe("Keep mine for a note that leaves the view while the answer is out", () => {
  const OTHER = "n2";
  // What the note's file holds, as Rust would read it.
  const file = { text: "" };

  function digestOf(text: string): string {
    let hash = 2166136261;
    for (let i = 0; i < text.length; i++) hash = Math.imul(hash ^ text.charCodeAt(i), 16777619) >>> 0;
    return hash.toString(16).padStart(8, "0").padEnd(64, "0");
  }

  /** A note opened from its file holding "hello", typed into, then asked about. */
  async function askedAfterTyping(moved: "lands" | "refused" = "lands") {
    file.text = "hello";
    api.noteDiskState.mockImplementation(async () => ({
      state: "described",
      disk: { hash: digestOf(file.text) },
    }));
    api.saveBufferContent.mockImplementation(async (_id: string, text: string) => {
      if (moved === "refused") throw new Error("ERR_FILE_CHANGED_ON_DISK: the file changed");
      file.text = text;
      return digestOf(text);
    });
    const store = createEditorStore();
    stores.push(store);
    const view = new EditorView({ state: EditorState.create({ doc: "hello" }), parent: document.body });
    views.push(view);
    store.registerView(view);
    store.setCurrentBufferId(NOTE);
    store.noteOpened(NOTE, view.state.doc, true);
    await vi.waitFor(() => expect(store.isTracked(NOTE)).toBe(true));
    active.win = { editor: store, tabs: { openFile: vi.fn() } };

    // EditorInstance's update listener.
    const type = (text: string) => {
      view.dispatch({ changes: { from: view.state.doc.length, insert: text } });
      store.noteDocChanged();
      store.noteEdited(NOTE, view.state.doc);
      store.scheduleAutosave(NOTE, view.state.doc, 1000);
    };

    type("1");
    file.text = "from another program";
    store.recordFileEvent(NOTE, "modified");
    return { store, view, type };
  }

  /** Keep mine, with Rust's answer held until the test lets it land. */
  function keepMine(): { answered: Promise<void>; land: (written: string) => void } {
    let resolve: (outcome: ResolveOutcome) => void = () => {};
    api.resolveExternalChange.mockImplementationOnce(
      () => new Promise<ResolveOutcome>((done) => (resolve = done)),
    );
    const answered = resolveNoteChange(NOTE, "keep_mine");
    const land = (written: string) => {
      file.text = written;
      resolve({ conflict_copy_path: null, content: null, disk_hash: digestOf(written) });
    };
    return { answered, land };
  }

  /** The tab switch: the outgoing held note keeps its text, then another note takes the view. */
  async function switchAway(store: ReturnType<typeof createEditorStore>, view: EditorView) {
    await flushAutosave(NOTE);
    store.keepTextOfRemoved(NOTE, view.state.doc.toString());
    const other = new EditorView({ state: EditorState.create({ doc: "other" }), parent: document.body });
    views.push(other);
    store.registerView(other);
    store.noteOpened(OTHER, other.state.doc, true);
    store.setCurrentBufferId(OTHER);
  }

  function written(): string[] {
    return api.saveBufferContent.mock.calls.map(([, text]) => text as string);
  }

  async function settled(): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, DIRTY_COMPARE_IDLE_MS + 30));
  }

  it.each(["lands", "refused"] as const)(
    "reads dirty when typing during the answer left with a write to the file found elsewhere (that write %s)",
    async (moved) => {
      const { store, view, type } = await askedAfterTyping(moved);
      const { answered, land } = keepMine();

      type("2");
      file.text = "";
      store.recordFileEvent(NOTE, "removed");
      await switchAway(store, view);
      file.text = "from another program";
      store.recordFileEvent(NOTE, "moved");
      await flushAutosave(NOTE);

      land("hello1");
      await answered;
      await settled();

      expect(written()).toEqual(["hello12"]);
      expect(file.text).toBe("hello1");
      expect(store.isDirty(NOTE)).toBe(true);
    },
  );

  it("reads clean when nothing was typed during the answer", async () => {
    const { store, view } = await askedAfterTyping();
    const { answered, land } = keepMine();

    await switchAway(store, view);

    land("hello1");
    await answered;
    await settled();

    expect(written()).toEqual([]);
    expect(store.isDirty(NOTE)).toBe(false);
  });

  it("writes the typing held during the answer and reads clean once that write lands", async () => {
    const { store, view, type } = await askedAfterTyping();
    const { answered, land } = keepMine();

    type("2");
    await switchAway(store, view);

    land("hello1");
    await answered;
    await flushAutosave(NOTE);
    await settled();

    expect(written()).toEqual(["hello12"]);
    expect(file.text).toBe("hello12");
    expect(store.isDirty(NOTE)).toBe(false);
  });
});
