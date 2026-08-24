import { describe, it, expect, vi, beforeEach } from "vitest";
import type { SearchBatch } from "../../types/search";

const h = vi.hoisted(() => ({
  workspaceIndexStatus: vi.fn(),
  searchWorkspaceFiles: vi.fn(),
  searchWorkspaceContent: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  workspaceIndexStatus: h.workspaceIndexStatus,
  searchWorkspaceFiles: h.searchWorkspaceFiles,
  searchWorkspaceContent: h.searchWorkspaceContent,
}));

import { workspaceSearchStore } from "../../stores/global/workspace-search";

function hit(path: string, line: number) {
  return { path, line, snippet: [{ text: path, matched: true }] };
}

function outcome(over: Partial<SearchBatch["outcome"]> = {}) {
  return {
    hit_count: 1,
    files_scanned: 1,
    truncated: false,
    cancelled: false,
    ...over,
  };
}

async function withWorkspace() {
  h.workspaceIndexStatus.mockResolvedValue({
    file_count: 10,
    truncated: false,
    has_workspace: true,
  });
  await workspaceSearchStore.refreshIndexStatus();
}

describe("workspaceSearchStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    workspaceSearchStore.reset();
    console.error = vi.fn();
  });

  it("records the index status and surfaces truncation", async () => {
    h.workspaceIndexStatus.mockResolvedValue({
      file_count: 200000,
      truncated: true,
      has_workspace: true,
    });
    const status = await workspaceSearchStore.refreshIndexStatus();
    expect(status.truncated).toBe(true);
    expect(workspaceSearchStore.indexStatus().file_count).toBe(200000);
  });

  it("falls back to an empty status when the backend refuses", async () => {
    h.workspaceIndexStatus.mockRejectedValue(new Error("no workspace"));
    const status = await workspaceSearchStore.refreshIndexStatus();
    expect(status.has_workspace).toBe(false);
  });

  it("returns no file hits for an empty query without calling the backend", async () => {
    expect(await workspaceSearchStore.searchFiles("")).toEqual([]);
    expect(h.searchWorkspaceFiles).not.toHaveBeenCalled();
  });

  // "No matches" and "the search never ran" are different answers: the store
  // rejects so the palette can tell the user which one it got.
  it("rejects when file search fails", async () => {
    h.searchWorkspaceFiles.mockRejectedValue(new Error("boom"));
    await expect(workspaceSearchStore.searchFiles("main")).rejects.toThrow("boom");
  });

  it("rejects when the content search fails", async () => {
    await withWorkspace();
    h.searchWorkspaceContent.mockRejectedValue(new Error("grep died"));
    await expect(
      workspaceSearchStore.streamContent("todo", vi.fn(), new AbortController().signal),
    ).rejects.toThrow("grep died");
  });

  it("does not grep when no workspace folder is open", async () => {
    const onBatch = vi.fn();
    await workspaceSearchStore.streamContent("todo", onBatch, new AbortController().signal);
    expect(h.searchWorkspaceContent).not.toHaveBeenCalled();
    expect(onBatch).not.toHaveBeenCalled();
  });

  it("delivers batches stamped with the live generation", async () => {
    await withWorkspace();
    h.searchWorkspaceContent.mockImplementation(
      async (_q: string, cb: (b: SearchBatch) => void) => {
        cb({ generation: 1, hits: [hit("a.rs", 1)], outcome: null });
        cb({ generation: 1, hits: [], outcome: outcome() });
      },
    );
    const onBatch = vi.fn();
    await workspaceSearchStore.streamContent("todo", onBatch, new AbortController().signal);
    expect(onBatch).toHaveBeenCalledTimes(2);
    expect(workspaceSearchStore.lastOutcome()?.hit_count).toBe(1);
  });

  it("discards batches from a superseded generation", async () => {
    await withWorkspace();
    h.searchWorkspaceContent.mockImplementationOnce(
      async (_q: string, cb: (b: SearchBatch) => void) => {
        cb({ generation: 7, hits: [hit("new.rs", 1)], outcome: null });
      },
    );
    const fresh = vi.fn();
    await workspaceSearchStore.streamContent("new", fresh, new AbortController().signal);
    expect(fresh).toHaveBeenCalledTimes(1);

    h.searchWorkspaceContent.mockImplementationOnce(
      async (_q: string, cb: (b: SearchBatch) => void) => {
        cb({ generation: 6, hits: [hit("stale.rs", 1)], outcome: null });
        cb({ generation: 8, hits: [hit("live.rs", 1)], outcome: null });
      },
    );
    const later = vi.fn();
    await workspaceSearchStore.streamContent("later", later, new AbortController().signal);
    expect(later).toHaveBeenCalledTimes(1);
    expect(later.mock.calls[0][0].hits[0].path).toBe("live.rs");
  });

  it("drops batches that arrive after the caller aborted", async () => {
    await withWorkspace();
    const controller = new AbortController();
    h.searchWorkspaceContent.mockImplementation(
      async (_q: string, cb: (b: SearchBatch) => void) => {
        controller.abort();
        cb({ generation: 20, hits: [hit("late.rs", 1)], outcome: null });
      },
    );
    const onBatch = vi.fn();
    await workspaceSearchStore.streamContent("todo", onBatch, controller.signal);
    expect(onBatch).not.toHaveBeenCalled();
  });

  it("surfaces a truncated grep outcome", async () => {
    await withWorkspace();
    h.searchWorkspaceContent.mockImplementation(
      async (_q: string, cb: (b: SearchBatch) => void) => {
        cb({ generation: 30, hits: [], outcome: outcome({ truncated: true, hit_count: 500 }) });
      },
    );
    await workspaceSearchStore.streamContent("todo", vi.fn(), new AbortController().signal);
    expect(workspaceSearchStore.lastOutcome()?.truncated).toBe(true);
  });
});
