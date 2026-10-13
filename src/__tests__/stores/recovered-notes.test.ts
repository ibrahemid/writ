import { describe, it, expect, vi, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(null),
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
  restoreNoteFile: vi.fn().mockResolvedValue(null),
  saveNoteTo: vi.fn().mockResolvedValue("digest"),
  recordUnsavedNotes: vi.fn().mockResolvedValue(undefined),
  discardUnsavedText: vi.fn().mockResolvedValue(undefined),
}));

import { createEditorStore } from "../../stores/window/editor-store";
import { applyRecoveredNotes } from "../../stores/window/recovered-notes";
import type { RecoveredBuffer } from "../../services/tauri";

let stores: ReturnType<typeof createEditorStore>[] = [];

function editor() {
  const store = createEditorStore();
  stores.push(store);
  return store;
}

afterEach(() => {
  for (const store of stores) store.stopSaveListener();
  stores = [];
});

const unmade = { name: "2026-10-13.txt", folder: "Writ" };

const restoredNote: RecoveredBuffer = {
  id: "restored",
  content: "written back",
  removed_on_disk: false,
};
const deletedNote: RecoveredBuffer = {
  id: "deleted",
  content: "typed after the file went",
  removed_on_disk: true,
  unmade_file: null,
};
const unmadeNote: RecoveredBuffer = {
  id: "unmade",
  content: "the first words",
  removed_on_disk: true,
  unmade_file: unmade,
};

describe("the notes the launch recovered", () => {
  it("brings a note whose first file the launch could not make up under a bar naming that file", () => {
    const store = editor();

    applyRecoveredNotes(store, [unmadeNote]);

    expect(store.isRemovedOnDisk("unmade")).toBe(true);
    expect(store.unmadeFileOf("unmade")).toEqual(unmade);
  });

  it("brings a note whose file was deleted up under the deleted-file bar", () => {
    const store = editor();

    applyRecoveredNotes(store, [deletedNote, { ...deletedNote, id: "older", unmade_file: undefined }]);

    expect(store.isRemovedOnDisk("deleted")).toBe(true);
    expect(store.unmadeFileOf("deleted")).toBeNull();
    expect(store.isRemovedOnDisk("older")).toBe(true);
    expect(store.unmadeFileOf("older")).toBeNull();
  });

  it("hands the editor each held note's text and the file it was to be", () => {
    const markRemovedOnDisk = vi.fn();

    applyRecoveredNotes({ markRemovedOnDisk }, [restoredNote, deletedNote, unmadeNote]);

    expect(markRemovedOnDisk.mock.calls).toEqual([
      ["deleted", "typed after the file went", undefined],
      ["unmade", "the first words", unmade],
    ]);
  });

  it("answers only the notes the launch restored, which the toast counts", () => {
    const restored = applyRecoveredNotes({ markRemovedOnDisk: vi.fn() }, [
      restoredNote,
      deletedNote,
      unmadeNote,
    ]);

    expect(restored.map((note) => note.id)).toEqual(["restored"]);
  });

  it("is what the window's launch hands the recovered notes to", () => {
    const app = readFileSync(resolve(process.cwd(), "src/App.tsx"), "utf8");

    expect(app).toContain("const recovered = await getRecoveredBuffers().catch(() => []);");
    expect(app).toContain("const restored = applyRecoveredNotes(win.editor, recovered);");
  });
});
