import { describe, it, expect, vi, beforeEach } from "vitest";
import type { BufferDocument } from "../../types/buffer";
import type { FileOpenResult } from "../../types/buffer";

function mockBuffer(title: string, id = "notices-buffer"): BufferDocument {
  return {
    id,
    title,
    filename: "notices.md",
    status: "active",
    language: null,
    source_path: "/writ/generated/Third-party licences.md",
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 1,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: true,
    size_bytes: 0,
    line_ending: "lf",
  };
}

function mockOpenResult(title: string, id = "notices-buffer"): FileOpenResult {
  return {
    doc: mockBuffer(title, id),
    mode: { kind: "Normal" },
    size_bytes: 0,
  };
}

const mocks = vi.hoisted(() => ({
  openThirdPartyNotices: vi.fn(),
  activeTabs: vi.fn(),
  registerOpenResult: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  openThirdPartyNotices: mocks.openThirdPartyNotices,
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    activeTabs: mocks.activeTabs,
    registerOpenResult: mocks.registerOpenResult,
  },
}));

import {
  openThirdPartyNoticesBuffer,
  THIRD_PARTY_NOTICES_TITLE,
} from "../../stores/global/notices";

describe("openThirdPartyNoticesBuffer", () => {
  beforeEach(() => {
    mocks.openThirdPartyNotices.mockReset().mockResolvedValue(mockOpenResult(THIRD_PARTY_NOTICES_TITLE));
    mocks.activeTabs.mockReset().mockReturnValue([]);
    mocks.registerOpenResult
      .mockReset()
      .mockImplementation((result: FileOpenResult) => ({
        doc: result.doc,
        existed: false,
        mode: result.mode,
      }));
  });

  it("opens the notices from the backend and registers the returned buffer", async () => {
    const { doc, reused } = await openThirdPartyNoticesBuffer();

    expect(mocks.openThirdPartyNotices).toHaveBeenCalledTimes(1);
    expect(mocks.registerOpenResult).toHaveBeenCalledWith(
      expect.objectContaining({ doc: expect.objectContaining({ id: "notices-buffer" }) }),
    );
    expect(doc.id).toBe("notices-buffer");
    expect(reused).toBe(false);
  });

  it("identifies the buffer by the file it opened from, under the data directory", async () => {
    const { doc } = await openThirdPartyNoticesBuffer();

    expect(doc.source_path).toBe("/writ/generated/Third-party licences.md");
    expect(doc.read_only).toBe(true);
  });

  it("reports reused when the returned buffer id was already an active tab", async () => {
    mocks.activeTabs.mockReturnValue([mockBuffer("Notes", "other"), mockBuffer(THIRD_PARTY_NOTICES_TITLE, "already-open")]);
    mocks.openThirdPartyNotices.mockResolvedValue(mockOpenResult(THIRD_PARTY_NOTICES_TITLE, "already-open"));

    const { doc, reused } = await openThirdPartyNoticesBuffer();

    expect(doc.id).toBe("already-open");
    expect(reused).toBe(true);
  });

  it("reports not reused for a buffer id that was not already an active tab", async () => {
    mocks.activeTabs.mockReturnValue([mockBuffer("Notes", "other")]);

    const { reused } = await openThirdPartyNoticesBuffer();

    expect(reused).toBe(false);
  });

  it("propagates a failure to read the notices without registering a buffer", async () => {
    mocks.openThirdPartyNotices.mockRejectedValue(new Error("missing"));

    await expect(openThirdPartyNoticesBuffer()).rejects.toThrow("missing");
    expect(mocks.registerOpenResult).not.toHaveBeenCalled();
  });
});
