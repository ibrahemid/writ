import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { BufferDocument, FileOpenResult } from "../../types/buffer";

// Reopening a closed file from History is an open: a file that grew past the
// size Writ opens without asking while it was closed is asked about before it
// is read, and one past the size Writ opens stays closed with the reason.

const SIXTY_MB = 60_000_000;
const PATH = "/logs/big.log";

function closedRow(overrides: Partial<BufferDocument> = {}): BufferDocument {
  return {
    id: "h1",
    title: "big.log",
    filename: "h1.txt",
    status: "history",
    language: null,
    source_path: PATH,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: new Date().toISOString(),
    read_only: false,
    size_bytes: 1_000,
    line_ending: "lf",
    encoding: "UTF-8",
    ...overrides,
  };
}

vi.mock("../../services/tauri", () => ({
  listActiveBuffers: vi.fn(async () => []),
  listHistory: vi.fn(async () => []),
  restoreBuffer: vi.fn(),
  restoreBufferConfirmed: vi.fn(),
  previewClose: vi.fn(async () => undefined),
}));

vi.mock("../../services/autosave", () => ({
  flushAutosave: vi.fn(async () => ({ ok: true, failures: [] })),
  keepUnsavedForRecovery: vi.fn(),
  cancelAutosave: vi.fn(),
  onAutosaveStart: vi.fn(() => () => {}),
  onAutosaveSuccess: vi.fn(() => () => {}),
  onAutosaveError: vi.fn(() => () => {}),
}));

vi.mock("../../stores/global/notifications", () => ({
  showToast: vi.fn(),
}));

import * as api from "../../services/tauri";
import { showToast } from "../../stores/global/notifications";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { largeFileOpen } from "../../stores/global/large-file-open";
import { createTabStore } from "../../stores/window/tab-store";
import type { DownloadStore } from "../../stores/window/download-store";

const mockedApi = vi.mocked(api);

function answer(result: FileOpenResult): void {
  mockedApi.restoreBuffer.mockResolvedValue(result as never);
}

function tabStore() {
  const downloads = {
    select: vi.fn(),
    start: vi.fn(async () => undefined),
  } as unknown as DownloadStore;
  return createTabStore({ registry: bufferRegistry, downloads });
}

beforeEach(async () => {
  vi.clearAllMocks();
  mockedApi.listActiveBuffers.mockResolvedValue([]);
  mockedApi.listHistory.mockResolvedValue([closedRow()]);
  await bufferRegistry.load();
});

afterEach(() => {
  for (let i = 0; i < 20 && largeFileOpen.state() !== null; i++) {
    largeFileOpen.respond(false);
    largeFileOpen.finish();
  }
});

describe("reopening a closed file from History", () => {
  it("asks about a file that grew past 50 MiB before reading it, then reopens it", async () => {
    answer({ doc: null, mode: { kind: "LargeFileConfirm" }, size_bytes: SIXTY_MB });
    const reopened = closedRow({ status: "active", closed_at: null, size_bytes: SIXTY_MB });
    mockedApi.restoreBufferConfirmed.mockResolvedValue({
      doc: reopened,
      mode: { kind: "LargeFile" },
      size_bytes: SIXTY_MB,
    } as never);
    mockedApi.listActiveBuffers.mockResolvedValue([reopened]);
    mockedApi.listHistory.mockResolvedValue([]);
    const tabs = tabStore();

    const reopening = tabs.restoreFromHistory("h1");
    await vi.waitFor(() =>
      expect(largeFileOpen.state()).toEqual({ phase: "asking", name: "big.log", sizeBytes: SIXTY_MB }),
    );
    expect(mockedApi.restoreBufferConfirmed).not.toHaveBeenCalled();
    largeFileOpen.respond(true);

    expect((await reopening)?.id).toBe("h1");
    expect(mockedApi.restoreBufferConfirmed).toHaveBeenCalledWith("h1");
    expect(tabs.activeTabId()).toBe("h1");
  });

  it("leaves the file in History and says nothing when the person cancels", async () => {
    answer({ doc: null, mode: { kind: "LargeFileConfirm" }, size_bytes: SIXTY_MB });
    const tabs = tabStore();

    const reopening = tabs.restoreFromHistory("h1");
    await vi.waitFor(() => expect(largeFileOpen.state()?.phase).toBe("asking"));
    largeFileOpen.respond(false);

    expect(await reopening).toBeNull();
    expect(mockedApi.restoreBufferConfirmed).not.toHaveBeenCalled();
    expect(tabs.activeTabId()).toBeNull();
    expect(showToast).not.toHaveBeenCalled();
    expect(bufferRegistry.historyList().map((b) => b.id)).toEqual(["h1"]);
  });

  it("says once why a file past the size Writ opens stays closed", async () => {
    answer({
      doc: null,
      mode: { kind: "Refused", reason: "ERR_OPEN_TOO_LARGE: over the limit" },
      size_bytes: 600_000_000,
    });
    const tabs = tabStore();

    expect(await tabs.restoreFromHistory("h1")).toBeNull();

    expect(showToast).toHaveBeenCalledTimes(1);
    expect(showToast).toHaveBeenCalledWith(
      "Could not open big.log: it is 600 MB, and Writ opens files up to 524 MB.",
      "error",
    );
    expect(tabs.activeTabId()).toBeNull();
    expect(largeFileOpen.state()).toBeNull();
  });

  it("reopens a file of an ordinary size at once", async () => {
    const reopened = closedRow({ status: "active", closed_at: null });
    answer({ doc: reopened, mode: { kind: "Normal" }, size_bytes: 1_000 });
    mockedApi.listActiveBuffers.mockResolvedValue([reopened]);
    mockedApi.listHistory.mockResolvedValue([]);
    const tabs = tabStore();

    expect((await tabs.restoreFromHistory("h1"))?.id).toBe("h1");

    expect(largeFileOpen.state()).toBeNull();
    expect(tabs.activeTabId()).toBe("h1");
    expect(bufferRegistry.activeTabs().map((b) => b.id)).toEqual(["h1"]);
  });
});
