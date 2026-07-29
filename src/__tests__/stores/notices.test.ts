import { describe, it, expect, vi, beforeEach } from "vitest";
import type { BufferDocument } from "../../types/buffer";

function mockBuffer(title: string, id = "notices-buffer"): BufferDocument {
  return {
    id,
    title,
    filename: "notices.md",
    status: "active",
    language: null,
    source_path: null,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 1,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 0,
  };
}

const mocks = vi.hoisted(() => ({
  readThirdPartyNotices: vi.fn(),
  saveBufferContentUnindexed: vi.fn(),
  saveBufferContent: vi.fn(),
  cancelAutosave: vi.fn(),
  createBuffer: vi.fn(),
  activeTabs: vi.fn(),
  readContent: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  readThirdPartyNotices: mocks.readThirdPartyNotices,
  saveBufferContentUnindexed: mocks.saveBufferContentUnindexed,
  saveBufferContent: mocks.saveBufferContent,
}));

vi.mock("../../services/autosave", () => ({
  cancelAutosave: mocks.cancelAutosave,
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    createBuffer: mocks.createBuffer,
    activeTabs: mocks.activeTabs,
    readContent: mocks.readContent,
  },
}));

import {
  openThirdPartyNoticesBuffer,
  THIRD_PARTY_NOTICES_TITLE,
} from "../../stores/global/notices";

describe("openThirdPartyNoticesBuffer", () => {
  beforeEach(() => {
    mocks.readThirdPartyNotices.mockReset().mockResolvedValue("# Third-party notices\n");
    mocks.saveBufferContentUnindexed.mockReset().mockResolvedValue(undefined);
    mocks.saveBufferContent.mockReset().mockResolvedValue(undefined);
    mocks.cancelAutosave.mockReset();
    mocks.createBuffer
      .mockReset()
      .mockImplementation((title: string) => Promise.resolve(mockBuffer(title)));
    mocks.activeTabs.mockReset().mockReturnValue([]);
    mocks.readContent.mockReset().mockResolvedValue("# Third-party notices\n");
  });

  it("writes the notices into a titled buffer", async () => {
    const { doc, reused } = await openThirdPartyNoticesBuffer();

    expect(mocks.createBuffer).toHaveBeenCalledWith(THIRD_PARTY_NOTICES_TITLE);
    expect(mocks.saveBufferContentUnindexed).toHaveBeenCalledWith(
      "notices-buffer",
      "# Third-party notices\n",
    );
    expect(doc.id).toBe("notices-buffer");
    expect(reused).toBe(false);
  });

  it("keeps the licence text out of the search index", async () => {
    await openThirdPartyNoticesBuffer();

    expect(mocks.saveBufferContent).not.toHaveBeenCalled();
  });

  it("reuses the open notices buffer instead of creating a second one", async () => {
    const open = mockBuffer(THIRD_PARTY_NOTICES_TITLE, "already-open");
    mocks.activeTabs.mockReturnValue([mockBuffer("Notes", "other"), open]);

    const { doc, reused } = await openThirdPartyNoticesBuffer();

    expect(mocks.createBuffer).not.toHaveBeenCalled();
    expect(doc.id).toBe("already-open");
    expect(reused).toBe(true);
  });

  it("refreshes the reused buffer and drops the edits queued against it", async () => {
    mocks.activeTabs.mockReturnValue([mockBuffer(THIRD_PARTY_NOTICES_TITLE, "already-open")]);
    mocks.readThirdPartyNotices.mockResolvedValue("# Regenerated\n");

    await openThirdPartyNoticesBuffer();

    expect(mocks.cancelAutosave).toHaveBeenCalledWith("already-open");
    expect(mocks.saveBufferContentUnindexed).toHaveBeenCalledWith(
      "already-open",
      "# Regenerated\n",
    );
  });

  it("leaves a buffer the user named the same thing alone", async () => {
    mocks.activeTabs.mockReturnValue([mockBuffer(THIRD_PARTY_NOTICES_TITLE, "user-notes")]);
    mocks.readContent.mockResolvedValue("things to check before shipping\n");

    const { doc, reused } = await openThirdPartyNoticesBuffer();

    expect(reused).toBe(false);
    expect(doc.id).toBe("notices-buffer");
    expect(mocks.saveBufferContentUnindexed).not.toHaveBeenCalledWith(
      "user-notes",
      expect.anything(),
    );
    expect(mocks.cancelAutosave).not.toHaveBeenCalled();
  });

  it("creates no buffer when the notices cannot be read", async () => {
    mocks.readThirdPartyNotices.mockRejectedValue(new Error("missing"));

    await expect(openThirdPartyNoticesBuffer()).rejects.toThrow("missing");
    expect(mocks.createBuffer).not.toHaveBeenCalled();
    expect(mocks.saveBufferContentUnindexed).not.toHaveBeenCalled();
  });
});
