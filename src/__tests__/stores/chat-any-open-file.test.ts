import { describe, it, expect, vi, beforeEach } from "vitest";

// A file open in a tab is context whether or not it sits in the notes folder,
// and outside the folder there is no root to strip: the key is the whole path.
// What this pins down is that the pane holds such a file by that path alone.

const mocks = vi.hoisted(() => ({
  chatAttachedSizes: vi.fn(),
  activeTabs: vi.fn<() => { id: string; source_path: string | null; size_bytes: number }[]>(
    () => [],
  ),
  stateOf: vi.fn<(id: string) => string>(() => "clean"),
  activeTabId: vi.fn<() => string | null>(() => null),
  root: vi.fn<() => string | null>(() => "/notes"),
}));

vi.mock("../../services/tauri", () => ({
  chatState: vi.fn(),
  chatAttachedSizes: mocks.chatAttachedSizes,
  chatList: vi.fn().mockResolvedValue([]),
  chatOpen: vi.fn(),
  chatNew: vi.fn(),
  chatRename: vi.fn(),
  chatDelete: vi.fn(),
  chatRenderReply: vi.fn(),
  chatSend: vi.fn(),
  chatStop: vi.fn(),
  chatApplyProposal: vi.fn(),
  chatDiscardProposal: vi.fn(),
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: mocks.activeTabs },
}));

vi.mock("../../stores/global/save-status", () => ({
  saveStatusStore: { stateOf: mocks.stateOf },
}));

vi.mock("../../stores/global/link", () => ({
  linkStore: { notePathsInFolder: vi.fn().mockResolvedValue([]) },
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => ({ tabs: { activeTabId: mocks.activeTabId } }) },
}));

vi.mock("../../stores/global/notes", () => ({
  notesStore: {
    root: mocks.root,
    contains: (path: string) => {
      const base = mocks.root();
      return base !== null && path.startsWith(`${base}/`);
    },
  },
}));

import { chatStore } from "../../stores/global/chat";

const README = "/elsewhere/repo/README.md";
const VENDORED = "/vendor/elsewhere/repo/README.md";

beforeEach(() => {
  mocks.chatAttachedSizes.mockReset().mockImplementation(async (paths: string[]) =>
    // Rust keys a file outside the folder by its own absolute path.
    paths.map((path) => ({ path, key: path.startsWith("/notes/") ? path.slice(7) : path, bytes: 9 })),
  );
  mocks.activeTabs.mockReset().mockReturnValue([]);
  mocks.stateOf.mockReset().mockReturnValue("clean");
  mocks.activeTabId.mockReset().mockReturnValue(null);
  mocks.root.mockReset().mockReturnValue("/notes");
  chatStore.reset();
  for (const note of chatStore.attachments()) chatStore.detach(note.path);
});

function tab(id: string, path: string) {
  return { id, note: { path, name: path.split("/").pop() ?? path, bytes: 9 } };
}

describe("a file the chat reaches through its tab", () => {
  it("a tab outside the notes folder gets a chip keyed by its path", async () => {
    await chatStore.followTab(tab("t1", README));

    const chip = chatStore.attachments()[0];
    expect(chip.key).toBe(README);
    expect(chip.name).toBe("README.md");
    expect(chip.auto).toBe(true);
  });

  it("attaching an outside path twice leaves one chip", async () => {
    await chatStore.followTab(tab("t1", README));
    await chatStore.attachByPath(README);

    expect(chatStore.attachments().map((note) => note.path)).toEqual([README]);
  });

  it("a dirty outside tab is found by its exact path", async () => {
    mocks.activeTabs.mockReturnValue([
      { id: "t9", source_path: VENDORED, size_bytes: 9 },
      { id: "t1", source_path: README, size_bytes: 9 },
    ]);
    await chatStore.attachByPath(README);

    mocks.stateOf.mockImplementation((id: string) => (id === "t1" ? "dirty" : "clean"));
    expect((await chatStore.attachedOnDisk())[0].dirty).toBe(true);

    // A vendored copy of the same file name is a different file, so its
    // unsaved text is not this chip's.
    mocks.stateOf.mockImplementation((id: string) => (id === "t9" ? "dirty" : "clean"));
    expect((await chatStore.attachedOnDisk())[0].dirty).toBe(false);
  });

  it("offers an open tab outside the folder to the mention list", () => {
    mocks.activeTabs.mockReturnValue([
      { id: "t1", source_path: README, size_bytes: 9 },
      { id: "t2", source_path: "/notes/Launch.md", size_bytes: 9 },
      { id: "t3", source_path: null, size_bytes: 0 },
    ]);

    expect(chatStore.openTabCandidates("read", 8)).toEqual([
      { path: README, name: "README.md" },
    ]);
  });
});
