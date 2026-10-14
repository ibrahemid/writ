import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { BufferDocument, FileOpenResult } from "../../types/buffer";

// Two files over the size Writ opens without asking, asked about at once: the
// later question waits until the earlier one is answered and its file is
// shown. No open is answered for the person, and no progress gives way to a
// question.

const SIXTY_MB = 60_000_000;
const SEVENTY_MB = 70_000_000;

function row(path: string): BufferDocument {
  return {
    id: `row:${path}`,
    title: path.split("/").pop() ?? path,
    filename: "row.txt",
    status: "active",
    language: null,
    source_path: path,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: SIXTY_MB,
    line_ending: "lf",
    encoding: "UTF-8",
  };
}

vi.mock("../../services/tauri", () => ({
  openFile: vi.fn(
    async (): Promise<FileOpenResult> => ({
      doc: null,
      mode: { kind: "LargeFileConfirm" },
      size_bytes: SIXTY_MB,
    }),
  ),
  openFileConfirmed: vi.fn(
    async (path: string): Promise<FileOpenResult> => ({
      doc: row(path),
      mode: { kind: "LargeFile" },
      size_bytes: SIXTY_MB,
    }),
  ),
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

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

import { largeFileOpen } from "../../stores/global/large-file-open";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { createTabStore } from "../../stores/window/tab-store";
import type { DownloadStore } from "../../stores/window/download-store";
import * as api from "../../services/tauri";

function asking(name: string, sizeBytes: number) {
  return { phase: "asking", name, sizeBytes };
}

// Answers every question still up or waiting with Cancel, and ends any
// progress, so no test leaves one behind for the next.
function drain(): void {
  for (let i = 0; i < 20 && largeFileOpen.state() !== null; i++) {
    largeFileOpen.respond(false);
    largeFileOpen.finish();
  }
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(drain);

describe("a question asked while another is up", () => {
  it("waits for the first answer, then is asked", async () => {
    const first = largeFileOpen.ask("a.log", SIXTY_MB);
    const second = largeFileOpen.ask("b.log", SEVENTY_MB);

    expect(largeFileOpen.state()).toEqual(asking("a.log", SIXTY_MB));

    largeFileOpen.respond(false);
    await expect(first).resolves.toBe(false);
    expect(largeFileOpen.state()).toEqual(asking("b.log", SEVENTY_MB));

    largeFileOpen.respond(true);
    await expect(second).resolves.toBe(true);
  });

  it("waits for the progress of an Open to end, then is asked", async () => {
    const first = largeFileOpen.ask("a.log", SIXTY_MB);
    largeFileOpen.respond(true);
    await expect(first).resolves.toBe(true);
    largeFileOpen.handOff("a");

    const second = largeFileOpen.ask("b.log", SEVENTY_MB);
    expect(largeFileOpen.state()).toMatchObject({ phase: "opening", name: "a.log" });

    largeFileOpen.finishFor("b");
    expect(largeFileOpen.state()).toMatchObject({ phase: "opening", name: "a.log" });

    largeFileOpen.finishFor("a");
    expect(largeFileOpen.state()).toEqual(asking("b.log", SEVENTY_MB));
    largeFileOpen.respond(false);
    await expect(second).resolves.toBe(false);
  });

  it("is asked in the order it was asked", async () => {
    const answers = ["a.log", "b.log", "c.log"].map((name) => largeFileOpen.ask(name, SIXTY_MB));

    const seen: string[] = [];
    for (let i = 0; i < 3; i++) {
      const current = largeFileOpen.state();
      if (current?.phase !== "asking") throw new Error("no question up");
      seen.push(current.name);
      largeFileOpen.respond(false);
    }

    expect(seen).toEqual(["a.log", "b.log", "c.log"]);
    await expect(Promise.all(answers)).resolves.toEqual([false, false, false]);
    expect(largeFileOpen.state()).toBeNull();
  });
});

describe("the tab store's opens of two large files at once", () => {
  function tabStore() {
    const downloads = {
      select: vi.fn(),
      start: vi.fn(async () => undefined),
    } as unknown as DownloadStore;
    return createTabStore({ registry: bufferRegistry, downloads });
  }

  it("asks about a file opened in front once one opened behind it is done", async () => {
    const tabs = tabStore();

    const behind = tabs.openPath("/inbox/arrived.log", { activate: false });
    await vi.waitFor(() => expect(largeFileOpen.state()).toEqual(asking("arrived.log", SIXTY_MB)));
    const front = tabs.openPath("/logs/mine.log");
    await vi.waitFor(() => expect(api.openFile).toHaveBeenCalledTimes(2));
    for (let i = 0; i < 20; i++) await Promise.resolve();
    expect(largeFileOpen.state()).toEqual(asking("arrived.log", SIXTY_MB));

    largeFileOpen.respond(true);
    const arrived = await behind;
    expect(arrived).toMatchObject({ kind: "opened", doc: { id: "row:/inbox/arrived.log" } });
    expect(tabs.activeTabId()).toBeNull();

    await vi.waitFor(() => expect(largeFileOpen.state()).toEqual(asking("mine.log", SIXTY_MB)));
    largeFileOpen.respond(true);
    const mine = await front;
    expect(mine).toMatchObject({ kind: "opened", doc: { id: "row:/logs/mine.log" } });
    expect(tabs.activeTabId()).toBe("row:/logs/mine.log");
  });
});
