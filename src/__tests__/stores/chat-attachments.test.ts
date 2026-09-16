import { describe, it, expect, vi, beforeEach } from "vitest";

// The chips are the whole of what a message carries (ADR-031 rule 2.5), so
// what this pins down is the set: what adds to it, what replaces in it, what
// clears it, and whether each chip states the note as it stands on disk.

const mocks = vi.hoisted(() => ({
  chatList: vi.fn(),
  chatOpen: vi.fn(),
  chatNew: vi.fn(),
  chatRename: vi.fn(),
  chatDelete: vi.fn(),
  chatRenderReply: vi.fn(),
  chatSend: vi.fn(),
  chatStop: vi.fn(),
  chatApplyProposal: vi.fn(),
  chatDiscardProposal: vi.fn(),
  chatAttachedSizes: vi.fn(),
  activeTabs: vi.fn<() => { id: string; source_path: string | null; size_bytes: number }[]>(
    () => [],
  ),
  stateOf: vi.fn<(id: string) => string>(() => "clean"),
  activeTabId: vi.fn<() => string | null>(() => null),
}));

vi.mock("../../services/tauri", () => ({
  chatState: vi.fn(),
  chatAttachedSizes: mocks.chatAttachedSizes,
  chatList: mocks.chatList,
  chatOpen: mocks.chatOpen,
  chatNew: mocks.chatNew,
  chatRename: mocks.chatRename,
  chatDelete: mocks.chatDelete,
  chatRenderReply: mocks.chatRenderReply,
  chatSend: mocks.chatSend,
  chatStop: mocks.chatStop,
  chatApplyProposal: mocks.chatApplyProposal,
  chatDiscardProposal: mocks.chatDiscardProposal,
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: mocks.activeTabs },
}));

vi.mock("../../stores/global/save-status", () => ({
  saveStatusStore: { stateOf: mocks.stateOf },
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => ({ tabs: { activeTabId: mocks.activeTabId } }) },
}));

import { chatStore, chipLabel } from "../../stores/global/chat";
import type { ChatConversation, ChatProposal } from "../../services/tauri";

const LAUNCH = "/notes/Ideas/Launch.md";
const OTHER = "/notes/Other.md";

const PROPOSAL: ChatProposal = {
  path: "Ideas/Launch.md",
  summary: "Fold the two intros together",
  before_hash: "before",
  new_content: "one intro, folded",
  hunks: [],
  status: "pending",
};

function conversation(id: string, turns: ChatConversation["turns"] = []): ChatConversation {
  return {
    id,
    title: id,
    created_at: "2026-09-16T10:00:00+00:00",
    updated_at: "2026-09-16T10:00:00+00:00",
    provider: "ollama",
    model: "a-model",
    turns,
  };
}

function sizeOf(path: string, bytes: number) {
  return { path, key: path.replace("/notes/", ""), bytes };
}

beforeEach(() => {
  for (const mock of Object.values(mocks)) mock.mockReset();
  mocks.chatList.mockResolvedValue([]);
  mocks.chatOpen.mockImplementation(async (id: string) => conversation(id));
  mocks.chatRenderReply.mockImplementation(async (text: string) => `<p>${text}</p>`);
  mocks.chatSend.mockResolvedValue({ conversation_id: "c1", request_id: "r", attached: [] });
  mocks.chatAttachedSizes.mockImplementation(async (paths: string[]) =>
    paths.map((path) => sizeOf(path, 12)),
  );
  mocks.activeTabs.mockReturnValue([]);
  mocks.stateOf.mockReturnValue("clean");
  mocks.activeTabId.mockReturnValue(null);
  chatStore.reset();
  for (const note of chatStore.attachments()) chatStore.detach(note.path);
});

describe("the set a message carries", () => {
  it("reopening the pane replaces the auto chip", async () => {
    await chatStore.attachAuto({ path: LAUNCH, name: "Launch.md", bytes: 12 });
    expect(chatStore.attachments().map((note) => note.path)).toEqual([LAUNCH]);

    await chatStore.attachAuto({ path: OTHER, name: "Other.md", bytes: 12 });

    expect(chatStore.attachments().map((note) => note.path)).toEqual([OTHER]);
    expect(chatStore.attachments()[0].auto).toBe(true);
  });

  it("leaves a chip a person added where it is", async () => {
    await chatStore.attachByPath(LAUNCH);
    await chatStore.attachAuto({ path: OTHER, name: "Other.md", bytes: 12 });

    expect(chatStore.attachments().map((note) => note.path)).toEqual([LAUNCH, OTHER]);
    expect(chatStore.attachments()[0].auto).toBeUndefined();
  });

  it("switching conversations clears chips", async () => {
    await chatStore.open("c1");
    await chatStore.attachByPath(LAUNCH);
    expect(chatStore.attachments()).toHaveLength(1);

    await chatStore.open("c2");

    expect(chatStore.attachments()).toEqual([]);
  });

  it("names a chip by its folder-relative key, with the folders above elided", async () => {
    await chatStore.attachByPath("/notes/Archive/Ideas/Launch.md");
    const chip = chatStore.attachments()[0];

    expect(chip.key).toBe("Archive/Ideas/Launch.md");
    expect(chipLabel(chip)).toBe("…/Ideas/Launch.md");
  });
});

describe("what a chip states", () => {
  it("chip sizes refresh after a changed apply", async () => {
    mocks.chatOpen.mockResolvedValue(
      conversation("c1", [
        { role: "user", content: "tighten it", attachments: [], proposals: [] },
        { role: "assistant", content: "here it is", attachments: [], proposals: [PROPOSAL] },
      ]),
    );
    await chatStore.open("c1");
    await chatStore.attachByPath(LAUNCH);
    expect(chatStore.attachments()[0].bytes).toBe(12);

    mocks.chatApplyProposal.mockResolvedValue({
      path: "Ideas/Launch.md",
      hash: "after",
      bytes: 80,
      changed: true,
    });
    mocks.chatAttachedSizes.mockImplementation(async (paths: string[]) =>
      paths.map((path) => sizeOf(path, 80)),
    );

    await chatStore.apply(1, PROPOSAL);
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(chatStore.attachments()[0].bytes).toBe(80);
  });

  it("an unreadable chip blocks send with its reason", async () => {
    await chatStore.attachByPath(LAUNCH);
    // One unreadable note refuses the whole batch, so the list is rebuilt one
    // note at a time to find which note and what Rust says about it.
    mocks.chatAttachedSizes.mockImplementation(async (paths: string[]) => {
      if (paths.includes(LAUNCH)) throw "Launch.md is larger than 2 MB.";
      return paths.map((path) => sizeOf(path, 12));
    });

    const resolved = await chatStore.attachedOnDisk();

    expect(resolved).toHaveLength(1);
    expect(resolved[0].state).toBe("unreadable");
    expect(resolved[0].reason).toBe("Launch.md is larger than 2 MB.");
  });

  it("one disk read per send", async () => {
    await chatStore.open("c1");
    await chatStore.attachByPath(LAUNCH);
    const resolved = await chatStore.attachedOnDisk();
    mocks.chatAttachedSizes.mockClear();

    chatStore.setDraft("what does it argue");
    await chatStore.send(resolved);

    expect(mocks.chatAttachedSizes).not.toHaveBeenCalled();
    expect(mocks.chatSend.mock.calls[0][2]).toEqual([LAUNCH]);
  });

  it("a dirty tab is marked as sending the saved version", async () => {
    mocks.activeTabs.mockReturnValue([{ id: "b1", source_path: LAUNCH, size_bytes: 12 }]);
    mocks.stateOf.mockReturnValue("dirty");
    await chatStore.attachByPath(LAUNCH);

    const resolved = await chatStore.attachedOnDisk();

    expect(resolved[0].dirty).toBe(true);
  });

  it("attaches the note in front", async () => {
    mocks.activeTabId.mockReturnValue("b1");
    mocks.activeTabs.mockReturnValue([{ id: "b1", source_path: LAUNCH, size_bytes: 12 }]);

    expect(await chatStore.addOpenNote()).toEqual({ ok: true, path: LAUNCH });
    expect(chatStore.attachments().map((note) => note.path)).toEqual([LAUNCH]);
  });

  // A tab with no file has no path to name and nothing on disk to read, so it
  // is refused in words rather than skipped in silence.
  it("says why a tab with no file cannot be attached", async () => {
    mocks.activeTabId.mockReturnValue("b1");
    mocks.activeTabs.mockReturnValue([{ id: "b1", source_path: null, size_bytes: 0 }]);

    expect(await chatStore.addOpenNote()).toEqual({ ok: false, reason: "unsaved" });
    expect(chatStore.attachments()).toEqual([]);
  });
});
