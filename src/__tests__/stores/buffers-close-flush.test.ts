import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { BufferDocument } from "../../types/buffer";

let tabIdCounter = 0;

function mockBuffer(overrides: Partial<BufferDocument> = {}): BufferDocument {
  tabIdCounter++;
  return {
    id: overrides.id ?? `buf-${tabIdCounter}`,
    title: overrides.title ?? `Buffer ${tabIdCounter}`,
    filename: overrides.filename ?? `buf-${tabIdCounter}.md`,
    status: overrides.status ?? "active",
    language: overrides.language ?? null,
    source_path: overrides.source_path ?? null,
    cursor_pos: overrides.cursor_pos ?? 0,
    scroll_pos: overrides.scroll_pos ?? 0,
    tab_order: overrides.tab_order ?? tabIdCounter,
    created_at: overrides.created_at ?? new Date().toISOString(),
    updated_at: overrides.updated_at ?? new Date().toISOString(),
    closed_at: overrides.closed_at ?? null,
  };
}

const callLog: string[] = [];

vi.mock("../../services/tauri", () => ({
  createBuffer: vi.fn().mockImplementation(() => Promise.resolve(mockBuffer())),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  closeBuffer: vi.fn().mockImplementation(async (id: string) => {
    callLog.push(`closeBuffer:${id}`);
  }),
  closeBuffers: vi.fn().mockImplementation(async (ids: string[]) => {
    callLog.push(`closeBuffers:${ids.join(",")}`);
  }),
  deleteBuffer: vi.fn().mockResolvedValue(undefined),
  restoreBuffer: vi.fn().mockResolvedValue(undefined),
  clearHistory: vi.fn().mockResolvedValue(undefined),
  renameBuffer: vi.fn().mockResolvedValue(undefined),
  saveBufferContent: vi.fn().mockImplementation(async (id: string, content: string) => {
    callLog.push(`saveBufferContent:${id}:${content}`);
  }),
}));

import { bufferStore } from "../../stores/buffers";
import { debouncedSave, cancelAutosave } from "../../services/autosave";
import * as api from "../../services/tauri";

const mockedApi = vi.mocked(api);

describe("bufferStore close flushes pending autosave", () => {
  beforeEach(async () => {
    vi.useFakeTimers();
    for (const b of bufferStore.activeTabs()) cancelAutosave(b.id);
    for (const b of bufferStore.historyList()) cancelAutosave(b.id);
    vi.clearAllMocks();
    callLog.length = 0;
    tabIdCounter = 0;
    mockedApi.listActiveBuffers.mockResolvedValue([]);
    mockedApi.listHistory.mockResolvedValue([]);
    await bufferStore.load();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("flushes pending autosave before closeTab calls closeBuffer", async () => {
    const doc = await bufferStore.createTab();

    debouncedSave(doc.id, "latest typed content", 300);

    await vi.advanceTimersByTimeAsync(200);

    expect(mockedApi.saveBufferContent).not.toHaveBeenCalled();

    await bufferStore.closeTab(doc.id);

    expect(mockedApi.saveBufferContent).toHaveBeenCalledWith(
      doc.id,
      "latest typed content",
    );

    const saveIdx = callLog.findIndex((e) => e.startsWith(`saveBufferContent:${doc.id}`));
    const closeIdx = callLog.findIndex((e) => e === `closeBuffer:${doc.id}`);
    expect(saveIdx).toBeGreaterThanOrEqual(0);
    expect(closeIdx).toBeGreaterThanOrEqual(0);
    expect(saveIdx).toBeLessThan(closeIdx);
  });

  it("flushes pending autosaves before closeOtherTabs calls closeBuffers", async () => {
    const keep = await bufferStore.createTab();
    const a = await bufferStore.createTab();
    const b = await bufferStore.createTab();

    debouncedSave(a.id, "a-pending", 300);
    debouncedSave(b.id, "b-pending", 300);
    await vi.advanceTimersByTimeAsync(50);

    await bufferStore.closeOtherTabs(keep.id);

    expect(mockedApi.saveBufferContent).toHaveBeenCalledWith(a.id, "a-pending");
    expect(mockedApi.saveBufferContent).toHaveBeenCalledWith(b.id, "b-pending");

    const closeIdx = callLog.findIndex((e) => e.startsWith("closeBuffers:"));
    const aIdx = callLog.findIndex((e) => e === `saveBufferContent:${a.id}:a-pending`);
    const bIdx = callLog.findIndex((e) => e === `saveBufferContent:${b.id}:b-pending`);
    expect(aIdx).toBeGreaterThanOrEqual(0);
    expect(bIdx).toBeGreaterThanOrEqual(0);
    expect(aIdx).toBeLessThan(closeIdx);
    expect(bIdx).toBeLessThan(closeIdx);
  });

  it("flushes pending autosaves before closeAllTabs calls closeBuffers", async () => {
    const a = await bufferStore.createTab();
    const b = await bufferStore.createTab();

    debouncedSave(a.id, "a-final", 300);
    debouncedSave(b.id, "b-final", 300);
    await vi.advanceTimersByTimeAsync(50);

    await bufferStore.closeAllTabs();

    expect(mockedApi.saveBufferContent).toHaveBeenCalledWith(a.id, "a-final");
    expect(mockedApi.saveBufferContent).toHaveBeenCalledWith(b.id, "b-final");

    const closeIdx = callLog.findIndex((e) => e.startsWith("closeBuffers:"));
    const aIdx = callLog.findIndex((e) => e === `saveBufferContent:${a.id}:a-final`);
    const bIdx = callLog.findIndex((e) => e === `saveBufferContent:${b.id}:b-final`);
    expect(aIdx).toBeLessThan(closeIdx);
    expect(bIdx).toBeLessThan(closeIdx);
  });

  it("persists an intentional empty buffer via closeTab flush (no length guard)", async () => {
    const doc = await bufferStore.createTab();

    debouncedSave(doc.id, "", 300);
    await vi.advanceTimersByTimeAsync(50);

    await bufferStore.closeTab(doc.id);

    expect(mockedApi.saveBufferContent).toHaveBeenCalledWith(doc.id, "");
  });
});
