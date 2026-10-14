import { describe, it, expect, vi, beforeEach } from "vitest";
import type { BufferDocument } from "../../types/buffer";

// Removing rows from Recent hides them at once and deletes them only when the
// Undo has gone; Undo shows them again and deletes nothing.

const h = vi.hoisted(() => ({
  history: [] as BufferDocument[],
  deleteBuffer: vi.fn(),
  clearHistory: vi.fn(),
  showToast: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  listActiveBuffers: vi.fn(async () => []),
  listHistory: vi.fn(async () => h.history),
  deleteBuffer: h.deleteBuffer,
  clearHistory: h.clearHistory,
}));
vi.mock("../../stores/global/notifications", () => ({ showToast: h.showToast }));

import { bufferRegistry } from "../../stores/global/buffer-registry";
import { removeFromRecent, removeOneFromRecent } from "../../stores/global/recent-removal";
import type { ToastRequest } from "../../stores/global/notifications";

function closed(id: string): BufferDocument {
  return {
    id,
    title: `${id}.md`,
    filename: `${id}.md`,
    status: "history",
    language: null,
    source_path: `/notes/${id}.md`,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "2026-10-14T09:00:00Z",
    updated_at: "2026-10-14T09:00:00Z",
    closed_at: "2026-10-14T09:00:00Z",
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
    encoding: "UTF-8",
  };
}

function toast(): ToastRequest {
  const calls = h.showToast.mock.calls;
  return calls[calls.length - 1][0] as ToastRequest;
}

const shown = () => bufferRegistry.historyList().map((b) => b.id);

beforeEach(async () => {
  h.deleteBuffer.mockReset().mockResolvedValue(undefined);
  h.clearHistory.mockReset().mockResolvedValue(undefined);
  h.showToast.mockReset();
  h.history = [closed("a"), closed("b"), closed("c")];
  await bufferRegistry.load();
});

describe("Remove from Recent", () => {
  it("hides the row and deletes it once the toast goes", async () => {
    removeOneFromRecent("b");
    expect(shown()).toEqual(["a", "c"]);
    expect(toast().text).toBe('Removed "b.md" from Recent');
    expect(h.deleteBuffer).not.toHaveBeenCalled();

    toast().onClose!(false);
    await vi.waitFor(() => expect(h.deleteBuffer).toHaveBeenCalledWith("b"));
    expect(h.clearHistory).not.toHaveBeenCalled();
    expect(shown()).toEqual(["a", "c"]);
  });

  it("shows the row again on Undo and deletes nothing", async () => {
    removeOneFromRecent("b");
    toast().onClose!(true);
    await toast().action!.run();
    expect(shown()).toEqual(["a", "b", "c"]);
    expect(h.deleteBuffer).not.toHaveBeenCalled();
  });
});

describe("Clear Recent", () => {
  it("clears the whole list in one call when nothing else was added", async () => {
    removeFromRecent(["a", "b", "c"], "Cleared Recent");
    expect(shown()).toEqual([]);
    toast().onClose!(false);
    await vi.waitFor(() => expect(h.clearHistory).toHaveBeenCalledTimes(1));
    expect(h.deleteBuffer).not.toHaveBeenCalled();
  });

  it("keeps a row it did not hide", async () => {
    removeFromRecent(["a", "b"], "Cleared Recent");
    toast().onClose!(false);
    await vi.waitFor(() => expect(h.deleteBuffer).toHaveBeenCalledTimes(2));
    expect(h.clearHistory).not.toHaveBeenCalled();
    expect(shown()).toEqual(["c"]);
  });
});
