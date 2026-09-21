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
  notePathsInFolder: vi.fn<(folder: string) => Promise<string[]>>(),
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

vi.mock("../../stores/global/link", () => ({
  linkStore: { notePathsInFolder: mocks.notePathsInFolder },
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => ({ tabs: { activeTabId: mocks.activeTabId } }) },
}));

import { chatStore, chipLabel, chipRows, folderChipLabel } from "../../stores/global/chat";
import type { ChatConversation, ChatProposal } from "../../services/tauri";

const LAUNCH = "/notes/Ideas/Launch.md";
const OTHER = "/notes/Other.md";
const ARCHIVE = "Archive";
const OLD = "/notes/Archive/Old.md";
const NESTED = "/notes/Archive/2025/Notes.md";

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
  mocks.notePathsInFolder.mockResolvedValue([]);
  chatStore.reset();
  for (const note of chatStore.attachments()) chatStore.detach(note.path);
});

function tab(id: string, path: string | null) {
  return {
    id,
    note: path === null ? null : { path, name: path.split("/").pop() ?? path, bytes: 12 },
  };
}

describe("the set a message carries", () => {
  it("a tab switch replaces the automatic chip", async () => {
    await chatStore.followTab(tab("t1", LAUNCH));
    expect(chatStore.attachments().map((note) => note.path)).toEqual([LAUNCH]);

    await chatStore.followTab(tab("t2", OTHER));

    expect(chatStore.attachments().map((note) => note.path)).toEqual([OTHER]);
    expect(chatStore.attachments()[0].auto).toBe(true);
  });

  it("leaves a chip a person added where it is", async () => {
    await chatStore.attachByPath(LAUNCH);
    await chatStore.followTab(tab("t2", OTHER));

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

  it("names a chip by the note's own name and keeps the folder in its key", async () => {
    await chatStore.attachByPath("/notes/Archive/Ideas/Launch.md");
    const chip = chatStore.attachments()[0];

    expect(chip.key).toBe("Archive/Ideas/Launch.md");
    expect(chipLabel(chip)).toBe("Launch.md");
  });
});

// Picking a folder attaches the notes under it, and the request carries them
// one per file: the folder is how they were chosen and how they are shown, not
// a second kind of thing that leaves the machine (ADR-031 rule 2.5).
describe("a folder the @ list offered", () => {
  it("attaches every note under it, the subfolders included", async () => {
    mocks.notePathsInFolder.mockResolvedValue([NESTED, OLD]);

    const result = await chatStore.attachFolder(ARCHIVE);

    expect(result).toEqual({ ok: true, notes: 2 });
    expect(mocks.notePathsInFolder).toHaveBeenCalledWith(ARCHIVE);
    expect(chatStore.attachments().map((note) => note.path)).toEqual([NESTED, OLD]);
    expect(chatStore.attachments().every((note) => note.viaFolder === ARCHIVE)).toBe(true);
  });

  it("reads as one chip carrying its notes and the bytes they add up to", async () => {
    mocks.notePathsInFolder.mockResolvedValue([NESTED, OLD]);
    mocks.chatAttachedSizes.mockImplementation(async (paths: string[]) =>
      paths.map((path) => sizeOf(path, path === OLD ? 30 : 12)),
    );

    await chatStore.attachFolder(ARCHIVE);
    await chatStore.attachByPath(LAUNCH);

    const rows = chipRows(chatStore.attachments());
    expect(rows).toHaveLength(2);
    expect(rows[0]).toMatchObject({ kind: "folder", folder: ARCHIVE, bytes: 42 });
    expect(rows[0].kind === "folder" && rows[0].notes.map((note) => note.path)).toEqual([
      NESTED,
      OLD,
    ]);
    expect(rows[1]).toMatchObject({ kind: "note" });
    expect(folderChipLabel("Archive/2025")).toBe("2025/");
  });

  it("refuses the whole folder when its notes would pass the ceiling", async () => {
    const many = Array.from({ length: 21 }, (_, at) => `/notes/Archive/n${at}.md`);
    mocks.notePathsInFolder.mockResolvedValue(many);
    // The ceiling and the sentence are Rust's, which is where the send reads
    // them too.
    mocks.chatAttachedSizes.mockRejectedValue("Attach at most 20 notes to one conversation.");

    const result = await chatStore.attachFolder(ARCHIVE);

    expect(result).toEqual({
      ok: false,
      reason: "Attach at most 20 notes to one conversation.",
    });
    expect(chatStore.attachments()).toEqual([]);
  });

  it("refuses the whole folder when one of its notes cannot be read", async () => {
    mocks.notePathsInFolder.mockResolvedValue([NESTED, OLD]);
    mocks.chatAttachedSizes.mockRejectedValue("Archive/Old.md is too large to attach.");

    const result = await chatStore.attachFolder(ARCHIVE);

    expect(result).toEqual({ ok: false, reason: "Archive/Old.md is too large to attach." });
    expect(chatStore.attachments()).toEqual([]);
  });

  it("refuses a folder the index holds no note for", async () => {
    mocks.notePathsInFolder.mockResolvedValue([]);

    const result = await chatStore.attachFolder(ARCHIVE);

    expect(result).toEqual({ ok: false, reason: "That folder holds no files." });
    expect(mocks.chatAttachedSizes).not.toHaveBeenCalled();
  });

  it("removing the folder chip removes every note it carried", async () => {
    mocks.notePathsInFolder.mockResolvedValue([NESTED, OLD]);
    await chatStore.attachFolder(ARCHIVE);
    await chatStore.attachByPath(LAUNCH);

    chatStore.detachFolder(ARCHIVE);

    expect(chatStore.attachments().map((note) => note.path)).toEqual([LAUNCH]);
  });

  it("keeps its count when the tab in front holds one of its notes", async () => {
    mocks.notePathsInFolder.mockResolvedValue([NESTED, OLD]);
    await chatStore.attachFolder(ARCHIVE);

    // The note in front is already one of the folder's, so the row says the
    // same thing it said: one note is one chip, whichever way it arrived.
    await chatStore.followTab(tab("t1", OLD));

    expect(chatStore.attachments().map((note) => note.path)).toEqual([NESTED, OLD]);
    const rows = chipRows(chatStore.attachments());
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ kind: "folder", folder: ARCHIVE, bytes: 24 });
  });

  it("leaves a note already picked by hand out of the folder's chip", async () => {
    await chatStore.attachByPath(OLD);
    mocks.notePathsInFolder.mockResolvedValue([NESTED, OLD]);

    const result = await chatStore.attachFolder(ARCHIVE);

    expect(result).toEqual({ ok: true, notes: 1 });
    expect(chatStore.attachments().map((note) => note.path)).toEqual([OLD, NESTED]);
    expect(chatStore.attachments()[0].viaFolder).toBeUndefined();
    expect(chatStore.attachments()[1].viaFolder).toBe(ARCHIVE);
  });
});

// The note in front follows the editor, so the chip row states the tab that
// is active rather than the tab the pane was opened over. Sending the chip
// away is remembered against that tab and forgotten as soon as another tab is
// focused.
describe("the note in front follows the editor", () => {
  it("a chip sent away stays away while that tab is in front", async () => {
    await chatStore.followTab(tab("t1", LAUNCH));
    chatStore.detach(LAUNCH);

    await chatStore.followTab(tab("t1", LAUNCH));

    expect(chatStore.attachments()).toEqual([]);
  });

  it("another tab in front brings the chip back", async () => {
    await chatStore.followTab(tab("t1", LAUNCH));
    chatStore.detach(LAUNCH);

    await chatStore.followTab(tab("t2", OTHER));
    expect(chatStore.attachments().map((note) => note.path)).toEqual([OTHER]);

    await chatStore.followTab(tab("t1", LAUNCH));
    expect(chatStore.attachments().map((note) => note.path)).toEqual([LAUNCH]);
  });

  it("a tab with no file leaves the chip row to the notes a person picked", async () => {
    await chatStore.attachByPath(OTHER);
    await chatStore.followTab(tab("t1", LAUNCH));
    expect(chatStore.attachments()).toHaveLength(2);

    await chatStore.followTab(tab("t3", null));

    expect(chatStore.attachments().map((note) => note.path)).toEqual([OTHER]);
  });

  it("the tab switched to last is the one chipped when two reads overlap", async () => {
    const waiting: (() => void)[] = [];
    mocks.chatAttachedSizes.mockImplementation(
      (paths: string[]) =>
        new Promise((resolve) => {
          waiting.push(() => resolve(paths.map((path) => sizeOf(path, 12))));
        }),
    );

    const first = chatStore.followTab(tab("t1", LAUNCH));
    const second = chatStore.followTab(tab("t2", OTHER));
    for (let round = 0; round < 3 && waiting.length > 0; round += 1) {
      for (const settle of waiting.splice(0, waiting.length)) settle();
      await Promise.resolve();
    }
    await Promise.all([first, second]);

    expect(chatStore.attachments().map((note) => note.path)).toEqual([OTHER]);
    expect(chatStore.attachments()[0].auto).toBe(true);
  });

  // The tab list is rebuilt whenever a note is opened, renamed or written
  // from outside, and each rebuild asks the chip row to follow the same tab
  // again. A tab already answered reads nothing.
  it("reads disk once for a note in front a person already picked", async () => {
    await chatStore.attachByPath(LAUNCH);
    await chatStore.followTab(tab("t1", LAUNCH));
    expect(chatStore.attachments()).toHaveLength(1);
    mocks.chatAttachedSizes.mockClear();

    await chatStore.followTab(tab("t1", LAUNCH));

    expect(mocks.chatAttachedSizes).not.toHaveBeenCalled();
    expect(chatStore.attachments()).toHaveLength(1);
  });

  it("a new chat asks for the tab in front again", async () => {
    await chatStore.followTab(tab("t1", LAUNCH));
    chatStore.detach(LAUNCH);
    const before = chatStore.attachGeneration();

    chatStore.newChat();
    expect(chatStore.attachGeneration()).toBe(before + 1);
    await chatStore.followTab(tab("t1", LAUNCH));

    expect(chatStore.attachments().map((note) => note.path)).toEqual([LAUNCH]);
  });

  it("another conversation asks for the tab in front again", async () => {
    await chatStore.open("c1");
    const before = chatStore.attachGeneration();

    await chatStore.open("c2");

    expect(chatStore.attachGeneration()).toBe(before + 1);
  });

  // The chips are the next message's. A send freezes what it carries, so a tab
  // switch while the reply arrives changes the row without touching the turn
  // already on its way.
  it("a tab switch mid-stream leaves the message in flight alone", async () => {
    await chatStore.open("c1");
    await chatStore.followTab(tab("t1", LAUNCH));
    const resolved = await chatStore.attachedOnDisk();
    chatStore.setDraft("what does it argue");
    await chatStore.send(resolved);
    expect(chatStore.status()).toBe("thinking");

    await chatStore.followTab(tab("t2", OTHER));

    expect(chatStore.attachments().map((note) => note.path)).toEqual([OTHER]);
    const sent = chatStore.messages().find((message) => message.role === "user");
    expect(sent?.attachments.map((note) => note.path)).toEqual([LAUNCH]);
    expect(mocks.chatSend.mock.calls[0][2]).toEqual([LAUNCH]);
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
