import { describe, it, expect, vi, beforeEach } from "vitest";

// The pane holds absolute source paths; the command that reads sizes off disk
// also names notes by their folder-relative key. The join has to be on the
// string the pane already holds, or every lookup misses and the dialog asking
// to send states whatever size the tab happened to record.

const mocks = vi.hoisted(() => ({
  chatAttachedSizes: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  chatState: vi.fn(),
  chatAttachedSizes: mocks.chatAttachedSizes,
  chatSend: vi.fn(),
  chatCancel: vi.fn(),
  chatApplyProposal: vi.fn(),
  chatDiscardProposal: vi.fn(),
}));

import { chatStore } from "../../stores/global/chat";

const LAUNCH = { path: "/notes/Launch.md", name: "Launch.md", bytes: 8 * 1024 };
const NESTED = { path: "/notes/Ideas/Later.md", name: "Later.md", bytes: 100 };

beforeEach(() => {
  mocks.chatAttachedSizes.mockReset();
  for (const note of chatStore.attachments()) chatStore.detach(note.path);
});

describe("the sizes the send dialog is given", () => {
  it("takes the size the command read off disk", async () => {
    chatStore.attach(LAUNCH);
    mocks.chatAttachedSizes.mockResolvedValue([
      { path: "/notes/Launch.md", key: "Launch.md", bytes: 16 * 1024 },
    ]);

    const refreshed = await chatStore.attachedOnDisk();
    expect(mocks.chatAttachedSizes).toHaveBeenCalledWith(["/notes/Launch.md"]);
    expect(refreshed).toEqual([{ ...LAUNCH, bytes: 16 * 1024 }]);
  });

  it("takes it for a note in a subfolder too", async () => {
    chatStore.attach(NESTED);
    mocks.chatAttachedSizes.mockResolvedValue([
      { path: "/notes/Ideas/Later.md", key: "Ideas/Later.md", bytes: 999 },
    ]);

    const refreshed = await chatStore.attachedOnDisk();
    expect(refreshed[0].bytes).toBe(999);
  });

  it("keeps the recorded size for a note the command did not answer about", async () => {
    chatStore.attach(LAUNCH);
    mocks.chatAttachedSizes.mockResolvedValue([]);

    const refreshed = await chatStore.attachedOnDisk();
    expect(refreshed[0].bytes).toBe(8 * 1024);
  });
});
