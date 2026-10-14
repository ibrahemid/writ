import { describe, it, expect, vi, beforeEach } from "vitest";
import { createRoot } from "solid-js";
import type { ContextPack } from "../../services/tauri";

// Market §2 #9: a file with its linked files, or a folder or tag slice, as one
// prompt with a token count and a preview.

const h = vi.hoisted(() => ({
  promptContextPack: vi.fn(),
  noteAllTags: vi.fn(),
  getNotesRoot: vi.fn(),
  writeClipboardText: vi.fn(),
  collectUnsavedContent: vi.fn(),
  activeTabs: [] as Array<{ id: string; source_path: string | null }>,
  activeTabId: null as string | null,
  toast: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  promptContextPack: h.promptContextPack,
  noteAllTags: h.noteAllTags,
  getNotesRoot: h.getNotesRoot,
  writeClipboardText: h.writeClipboardText,
  readClipboardText: vi.fn(),
}));
vi.mock("../../services/autosave", () => ({
  collectUnsavedContent: h.collectUnsavedContent,
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: () => h.activeTabs },
}));
vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({ tabs: { activeTabId: () => h.activeTabId } }),
  },
}));
vi.mock("../../stores/global/notifications", () => ({ showToast: h.toast }));

import { createContextPackStore } from "../../stores/global/context-pack";

function pack(paths: string[]): ContextPack {
  return {
    text: paths.map((path) => `<file path="${path}">\nx\n</file>`).join("\n\n"),
    tokens: 10 * paths.length,
    files: paths.map((path) => ({ path, tokens: 10 })),
    skipped: [],
    omitted: 0,
  };
}

function store() {
  return createRoot(createContextPackStore);
}

beforeEach(() => {
  vi.clearAllMocks();
  h.getNotesRoot.mockResolvedValue("/Users/me/Writ");
  h.noteAllTags.mockResolvedValue([
    { tag: "launch", count: 3 },
    { tag: "draft", count: 1 },
  ]);
  h.collectUnsavedContent.mockReturnValue([]);
  h.writeClipboardText.mockResolvedValue(undefined);
  h.activeTabs = [
    { id: "plan", source_path: "/Users/me/Writ/work/Plan.md" },
    { id: "budget", source_path: "/Users/me/Writ/work/Budget.md" },
    { id: "log", source_path: "/var/log/system.log" },
  ];
  h.activeTabId = "plan";
  h.promptContextPack.mockImplementation(async () => pack(["work/Plan.md", "work/Budget.md"]));
});

describe("the context pack", () => {
  it("opens on the file in front and the files it links to", async () => {
    const s = store();
    await s.open();

    expect(s.isOpen()).toBe(true);
    expect(s.kind()).toBe("linked");
    expect(h.promptContextPack).toHaveBeenCalledWith(
      { kind: "linked", path: "/Users/me/Writ/work/Plan.md" },
      [],
    );
    expect(s.pack()?.files.map((file) => file.path)).toEqual(["work/Plan.md", "work/Budget.md"]);
  });

  it("sends what the open tabs hold that is not written yet", async () => {
    h.collectUnsavedContent.mockReturnValue([
      { id: "budget", content: "typed a moment ago" },
      { id: "gone", content: "a tab that closed" },
    ]);
    const s = store();
    await s.open();

    expect(h.promptContextPack.mock.calls[0][1]).toEqual([
      { path: "/Users/me/Writ/work/Budget.md", text: "typed a moment ago" },
    ]);
  });

  it("offers the whole folder for a file outside it, which has no links the index knows", async () => {
    h.activeTabId = "log";
    const s = store();
    await s.open();

    expect(s.file()).toBeNull();
    expect(s.kind()).toBe("folder");
    expect(h.promptContextPack).toHaveBeenCalledWith(
      { kind: "folder", path: "/Users/me/Writ" },
      [],
    );
  });

  it("packs the folder the file is in, or a tag, when chosen", async () => {
    const s = store();
    await s.open();

    s.choose("folder");
    await vi.waitFor(() => expect(h.promptContextPack).toHaveBeenCalledTimes(2));
    expect(h.promptContextPack.mock.calls[1][0]).toEqual({
      kind: "folder",
      path: "/Users/me/Writ/work",
    });

    await vi.waitFor(() => expect(s.tag()).toBe("launch"));
    s.choose("tag");
    await vi.waitFor(() => expect(h.promptContextPack).toHaveBeenCalledTimes(3));
    expect(h.promptContextPack.mock.calls[2][0]).toEqual({ kind: "tag", tag: "launch" });

    s.chooseTag("draft");
    await vi.waitFor(() => expect(h.promptContextPack).toHaveBeenCalledTimes(4));
    expect(h.promptContextPack.mock.calls[3][0]).toEqual({ kind: "tag", tag: "draft" });
  });

  it("keeps the answer of the last choice when an earlier one arrives late", async () => {
    let answerLinked!: (value: ContextPack) => void;
    h.promptContextPack
      .mockImplementationOnce(() => new Promise<ContextPack>((r) => (answerLinked = r)))
      .mockImplementationOnce(async () => pack(["work/Plan.md"]));
    const s = store();
    const opening = s.open();
    await vi.waitFor(() => expect(h.promptContextPack).toHaveBeenCalledTimes(1));

    s.choose("folder");
    await vi.waitFor(() => expect(s.pack()?.files).toHaveLength(1));
    answerLinked(pack(["a.md", "b.md", "c.md"]));
    await opening;

    expect(s.pack()?.files).toHaveLength(1);
  });

  it("says so when the files could not be read", async () => {
    h.promptContextPack.mockRejectedValue("the file index could not be read");
    const s = store();
    await s.open();

    expect(s.failed()).toBe(true);
    expect(s.pack()).toBeNull();
    expect(await s.copy()).toBe(false);
  });

  it("copies the prompt, says how many files went in, and closes", async () => {
    const s = store();
    await s.open();

    expect(await s.copy()).toBe(true);
    expect(h.writeClipboardText).toHaveBeenCalledWith(pack(["work/Plan.md", "work/Budget.md"]).text);
    expect(h.toast).toHaveBeenCalledWith("Copied 2 files as a prompt", "success");
    expect(s.isOpen()).toBe(false);
  });

  it("copies nothing from a pack with no files", async () => {
    h.promptContextPack.mockResolvedValue(pack([]));
    const s = store();
    await s.open();

    expect(await s.copy()).toBe(false);
    expect(h.writeClipboardText).not.toHaveBeenCalled();
  });
});
