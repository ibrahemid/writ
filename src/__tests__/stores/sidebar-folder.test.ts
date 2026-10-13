import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  getNotesRoot: vi.fn(),
  getWorkspaceRoot: vi.fn(),
  clearWorkspaceRoot: vi.fn(async () => {}),
  updateConfig: vi.fn(async () => {}),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn().mockResolvedValue(() => {}),
}));

import { sidebarFolderStore } from "../../stores/global/sidebar-folder";
import { notesStore } from "../../stores/global/notes";
import { notesTreeStore } from "../../stores/global/notes-tree";
import { workspaceStore } from "../../stores/global/workspace";
import { configStore } from "../../stores/global/config";
import * as api from "../../services/tauri";

const mockedApi = vi.mocked(api);

async function folders(notes: string | null, opened: string | null): Promise<void> {
  if (notes === null) mockedApi.getNotesRoot.mockRejectedValue(new Error("unset"));
  else mockedApi.getNotesRoot.mockResolvedValue(notes);
  mockedApi.getWorkspaceRoot.mockResolvedValue(opened);
  await notesStore.load();
  await workspaceStore.hydrate();
}

beforeEach(async () => {
  vi.clearAllMocks();
  await workspaceStore.closeFolder();
});

afterEach(() => {
  configStore.setSidebarSectionHidden("folder", false);
});

describe("the sidebar's folder", () => {
  it("is nothing before either folder is known", async () => {
    await folders(null, null);

    expect(sidebarFolderStore.kind()).toBeNull();
    expect(sidebarFolderStore.tree()).toBeNull();
    expect(sidebarFolderStore.showsNotes()).toBe(false);
  });

  it("is the notes folder while no other folder is open", async () => {
    await folders("/Users/me/Writ", null);

    expect(sidebarFolderStore.kind()).toBe("notes");
    expect(sidebarFolderStore.tree()).toBe(notesTreeStore);
    expect(sidebarFolderStore.showsNotes()).toBe(true);
  });

  it("is the folder opened by hand while there is one", async () => {
    await folders("/Users/me/Writ", "/Users/me/code/app");

    expect(sidebarFolderStore.kind()).toBe("workspace");
    expect(sidebarFolderStore.tree()).toBe(workspaceStore);
  });

  it("goes back to the notes folder once the folder opened by hand is closed", async () => {
    await folders("/Users/me/Writ", "/Users/me/code/app");

    await workspaceStore.closeFolder();

    expect(sidebarFolderStore.tree()).toBe(notesTreeStore);
  });
});

describe("whether a tag filters the tree", () => {
  // A folder opened by hand is listed by the rule for a code folder, which
  // leaves out names the notes index reads, so even the notes folder opened by
  // hand can miss a file a tag names.
  it("lists under the tag for the notes folder itself opened by hand", async () => {
    await folders("/Users/me/Writ", "/Users/me/Writ");

    expect(sidebarFolderStore.showsNotes()).toBe(false);
  });

  it("lists under the tag for a folder opened by hand above the notes folder", async () => {
    await folders("/Users/me/Writ", "/Users/me");

    expect(sidebarFolderStore.showsNotes()).toBe(false);
  });

  it("lists under the tag for a folder unrelated to the notes folder", async () => {
    await folders("/Users/me/Writ", "/Users/me/code/app");

    expect(sidebarFolderStore.showsNotes()).toBe(false);
  });

  it("lists under the tag for a folder inside the notes folder, which holds only some of its files", async () => {
    await folders("/Users/me/Writ", "/Users/me/Writ/Projects");

    expect(sidebarFolderStore.showsNotes()).toBe(false);
  });

  it("lists under the tag while the Files section is switched off", async () => {
    await folders("/Users/me/Writ", null);

    configStore.setSidebarSectionHidden("folder", true);

    expect(sidebarFolderStore.showsNotes()).toBe(false);
  });
});
