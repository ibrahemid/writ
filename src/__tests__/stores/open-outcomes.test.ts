import { describe, it, expect, vi, beforeEach } from "vitest";
import type { BufferDocument, FileOpenResult } from "../../types/buffer";

// Every open has an answer: opened, declined (the person said Cancel),
// refused (with the reason a person reads) or not-downloaded. The 50-500 MiB
// question travels as data, so the path the person asked for is the path the
// confirmed open reads, whatever characters it holds.

const SIXTY_MB = 60_000_000;

let rowCounter = 0;

function row(path: string, overrides: Partial<BufferDocument> = {}): BufferDocument {
  rowCounter += 1;
  return {
    id: `row-${rowCounter}`,
    title: path.split(/[\\/]/).pop() ?? path,
    filename: `row-${rowCounter}.txt`,
    status: "active",
    language: null,
    source_path: path,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: rowCounter,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
    ...overrides,
  };
}

const answers = new Map<string, () => Promise<FileOpenResult>>();

vi.mock("../../services/tauri", () => ({
  openFile: vi.fn(async (path: string) => {
    const answer = answers.get(path);
    if (!answer) throw new Error(`no answer for ${path}`);
    return answer();
  }),
  openFileConfirmed: vi.fn(async (path: string) => ({
    doc: row(path, { size_bytes: SIXTY_MB }),
    mode: { kind: "LargeFile" },
    size_bytes: SIXTY_MB,
  })),
  previewClose: vi.fn(async () => undefined),
}));

vi.mock("../../services/autosave", () => ({
  flushAutosave: vi.fn(async () => ({ ok: true, failures: [] })),
  cancelAutosave: vi.fn(),
  onAutosaveStart: vi.fn(() => () => {}),
  onAutosaveSuccess: vi.fn(() => () => {}),
  onAutosaveError: vi.fn(() => () => {}),
}));

let openAnswer = true;

vi.mock("../../stores/global/large-file-open", () => ({
  largeFileOpen: {
    state: vi.fn(() => null),
    ask: vi.fn(async () => openAnswer),
    respond: vi.fn(),
    shown: vi.fn(async () => undefined),
    handOff: vi.fn(),
    finishFor: vi.fn(),
    finish: vi.fn(),
  },
}));

import { bufferRegistry, type OpenOutcome } from "../../stores/global/buffer-registry";
import { largeFileOpen } from "../../stores/global/large-file-open";
import * as api from "../../services/tauri";

const asks = vi.mocked(largeFileOpen.ask);
const confirmed = vi.mocked(api.openFileConfirmed);

function confirmTier(path: string, sizeBytes = SIXTY_MB): void {
  answers.set(path, async () => ({
    doc: null,
    mode: { kind: "LargeFileConfirm" },
    size_bytes: sizeBytes,
  }));
}

function expectOpened(outcome: OpenOutcome) {
  if (outcome.kind !== "opened") throw new Error(`expected opened, got ${outcome.kind}`);
  return outcome;
}

beforeEach(() => {
  answers.clear();
  openAnswer = true;
  vi.clearAllMocks();
});

describe("the 50-500 MiB question", () => {
  it.each([
    ["a Windows path", "C:\\Users\\me\\logs\\big.log", "big.log"],
    ["a verbatim Windows path", "\\\\?\\C:\\Users\\me\\logs\\big.log", "big.log"],
    ["a POSIX name holding a colon", "/Users/me/logs/10:30.log", "10:30.log"],
    ["a plain POSIX path", "/Users/me/logs/server.log", "server.log"],
  ])("states the size of %s and opens it by the path asked for", async (_case, path, name) => {
    confirmTier(path);

    const outcome = expectOpened(await bufferRegistry.openFile(path));

    expect(asks).toHaveBeenCalledWith(name, SIXTY_MB);
    expect(confirmed).toHaveBeenCalledWith(path);
    expect(outcome.doc.source_path).toBe(path);
    expect(outcome.mode).toEqual({ kind: "LargeFile" });
    expect(largeFileOpen.handOff).toHaveBeenCalledWith(outcome.doc.id);
  });

  it("asks before the confirmed open reads anything", async () => {
    const path = "/Users/me/logs/order.log";
    confirmTier(path);
    const order: string[] = [];
    asks.mockImplementationOnce(async () => {
      order.push("asked");
      return true;
    });
    confirmed.mockImplementationOnce(async (p: string) => {
      order.push("read");
      return { doc: row(p), mode: { kind: "LargeFile" }, size_bytes: SIXTY_MB };
    });

    await bufferRegistry.openFile(path);

    expect(order).toEqual(["asked", "read"]);
  });

  it("answers Cancel as declined, reads nothing and throws nothing", async () => {
    const path = "C:\\Users\\me\\logs\\cancel.log";
    confirmTier(path);
    openAnswer = false;

    const outcome = await bufferRegistry.openFile(path);

    expect(outcome).toEqual({ kind: "declined", path });
    expect(confirmed).not.toHaveBeenCalled();
    expect(bufferRegistry.activeTabs().some((b) => b.source_path === path)).toBe(false);
  });

  it("ends the progress when the confirmed open cannot read the file", async () => {
    const path = "/Users/me/logs/gone.log";
    confirmTier(path);
    confirmed.mockRejectedValueOnce("ERR_READ_FILE_MISSING: io error: not found");

    const outcome = await bufferRegistry.openFile(path);

    expect(outcome).toEqual({ kind: "refused", path, reason: "it is no longer there." });
    expect(largeFileOpen.finish).toHaveBeenCalled();
  });
});

describe("a refused open says why", () => {
  it("words the size refusal from the file's size", async () => {
    const path = "/Users/me/logs/dump.log";
    answers.set(path, async () => ({
      doc: null,
      mode: { kind: "Refused", reason: "ERR_OPEN_TOO_LARGE: 572.2 MiB is over the 500.0 MiB limit" },
      size_bytes: 600_000_000,
    }));

    const outcome = await bufferRegistry.openFile(path);

    expect(outcome).toEqual({
      kind: "refused",
      path,
      reason: "it is 600 MB, and Writ opens files up to 524 MB.",
    });
    expect(asks).not.toHaveBeenCalled();
  });

  it("words a read the filesystem refused from its code", async () => {
    const path = "/Users/me/locked.txt";
    answers.set(path, async () => {
      throw "ERR_READ_PERMISSION_DENIED: io error: Permission denied (os error 13)";
    });

    const outcome = await bufferRegistry.openFile(path);

    expect(outcome).toEqual({
      kind: "refused",
      path,
      reason: "you do not have permission to read it.",
    });
  });

  it("never shows a person the operating system's own sentence", async () => {
    const path = "/Users/me/odd.txt";
    answers.set(path, async () => {
      throw "database is locked";
    });

    const outcome = await bufferRegistry.openFile(path);

    expect(outcome.kind).toBe("refused");
    if (outcome.kind !== "refused") return;
    expect(outcome.reason).not.toContain("database");
    expect(outcome.reason.length).toBeGreaterThan(0);
  });
});

describe("a file whose bytes are elsewhere", () => {
  it("answers not-downloaded with the provider", async () => {
    const path = "/Users/me/iCloud/away.md";
    answers.set(path, async () => ({
      doc: null,
      mode: { kind: "NotDownloaded", path, provider: "iCloud Drive" },
      size_bytes: 10,
    }));

    expect(await bufferRegistry.openFile(path)).toEqual({
      kind: "not-downloaded",
      path,
      provider: "iCloud Drive",
    });
  });
});

describe("opening a file that is already open", () => {
  it("keeps the mode Rust reported when the file was opened", async () => {
    const path = "/Users/me/data/grew.csv";
    // Rust classified the file when it read it; the row's size is what a
    // later write made it. The reopen answers what the open reported.
    answers.set(path, async () => {
      const doc = row(path, { size_bytes: 6 * 1024 * 1024 });
      return { doc, mode: { kind: "Normal" }, size_bytes: doc.size_bytes };
    });

    const first = expectOpened(await bufferRegistry.openFile(path));
    const again = expectOpened(await bufferRegistry.openFile(path));

    expect(again.existed).toBe(true);
    expect(again.doc.id).toBe(first.doc.id);
    expect(again.mode).toEqual({ kind: "Normal" });
  });

  it("answers a path that spells an open file another way with that tab", async () => {
    const real = "/private/var/logs/big.log";
    const shown = row(real, { size_bytes: SIXTY_MB });
    answers.set(real, async () => ({ doc: shown, mode: { kind: "LargeFile" }, size_bytes: SIXTY_MB }));
    expectOpened(await bufferRegistry.openFile(real));
    // Rust resolves the other spelling to the same file and answers its tab.
    answers.set("/var/logs/big.log", async () => ({
      doc: shown,
      mode: { kind: "LargeFile" },
      size_bytes: SIXTY_MB,
    }));

    const again = expectOpened(await bufferRegistry.openFile("/var/logs/big.log"));

    expect(again.existed).toBe(true);
    expect(again.doc.id).toBe(shown.id);
    expect(asks).not.toHaveBeenCalled();
  });
});
