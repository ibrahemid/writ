import { describe, it, expect, vi, beforeEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const mocks = vi.hoisted(() => ({
  pickNotesFolder: vi.fn(),
  getNotesFolder: vi.fn(),
  getNotesRoot: vi.fn(),
  showNotesFolderInFinder: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  pickNotesFolder: mocks.pickNotesFolder,
  getNotesFolder: mocks.getNotesFolder,
  getNotesRoot: mocks.getNotesRoot,
  showNotesFolderInFinder: mocks.showNotesFolderInFinder,
}));

vi.mock("../../services/clipboard", () => ({ writeClipboardText: vi.fn() }));

import { notesStore } from "../../stores/global/notes";

describe("notes folder move", () => {
  beforeEach(() => {
    mocks.pickNotesFolder.mockReset();
    mocks.getNotesFolder.mockReset().mockResolvedValue({
      path: "/home/user/Notes",
      display_path: "~/Notes",
      fallback: null,
      sync_provider: null,
    });
  });

  // The Move control is the whole of O3's answer to a folder in the wrong
  // place, and `pick_notes_folder` is the one command that picks and moves in a
  // single step. The service is the only place the command name is written.
  it("move_calls_pick_notes_folder", async () => {
    mocks.pickNotesFolder.mockResolvedValue({ new_root: "/home/user/Notes", collided: [] });

    const outcome = await notesStore.move();

    expect(mocks.pickNotesFolder).toHaveBeenCalledTimes(1);
    expect(outcome?.new_root).toBe("/home/user/Notes");
    expect(mocks.getNotesFolder).toHaveBeenCalledTimes(1);
  });

  it("pick_notes_folder_is_the_command_the_service_invokes", () => {
    const source = readFileSync(resolve(process.cwd(), "src/services/tauri.ts"), "utf8");
    expect(source).toMatch(
      /export async function pickNotesFolder\([^)]*\)[^{]*\{\s*return invoke\("pick_notes_folder"\);/,
    );
  });

  it("move_leaves_the_folder_alone_when_the_picker_was_cancelled", async () => {
    mocks.pickNotesFolder.mockResolvedValue(null);

    expect(await notesStore.move()).toBeNull();
    expect(mocks.getNotesFolder).not.toHaveBeenCalled();
  });

  it("move_does_not_refresh_when_the_target_already_held_those_names", async () => {
    mocks.pickNotesFolder.mockResolvedValue({
      new_root: "/home/user/Notes",
      collided: ["note.md"],
    });

    const outcome = await notesStore.move();

    expect(outcome?.collided).toEqual(["note.md"]);
    expect(mocks.getNotesFolder).not.toHaveBeenCalled();
  });
});
