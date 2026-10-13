import { describe, it, expect, vi, beforeEach } from "vitest";
import type { BufferDocument } from "../../types/buffer";

// One reporter turns an open's answer into what the person is told: a refusal
// with its reason, nothing for Cancel, and one line for a gesture that opened
// many files.

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

vi.mock("../../services/autosave", () => ({
  flushAutosave: vi.fn(async () => ({ ok: true, failures: [] })),
  keepUnsavedForRecovery: vi.fn(),
  cancelAutosave: vi.fn(),
  onAutosaveStart: vi.fn(() => () => {}),
  onAutosaveSuccess: vi.fn(() => () => {}),
  onAutosaveError: vi.fn(() => () => {}),
}));

vi.mock("../../stores/global/large-file-open", () => ({
  largeFileOpen: {
    state: vi.fn(() => null),
    ask: vi.fn(async () => true),
    respond: vi.fn(),
    shown: vi.fn(async () => undefined),
    handOff: vi.fn(),
    finishFor: vi.fn(),
    finish: vi.fn(),
  },
}));

import { showToast } from "../../components/Notifications/Toast";
import { flushAutosave } from "../../services/autosave";
import { reportOpenOutcomes } from "../../stores/global/open-report";
import { largeFileOpen } from "../../stores/global/large-file-open";
import { createTabStore } from "../../stores/window/tab-store";
import type { BufferRegistry, OpenOutcome } from "../../stores/global/buffer-registry";
import type { DownloadStore } from "../../stores/window/download-store";

const toast = vi.mocked(showToast);

function row(id: string, path: string): BufferDocument {
  return {
    id,
    title: path.split("/").pop() ?? path,
    filename: `${id}.txt`,
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
    size_bytes: 0,
    line_ending: "lf",
  };
}

const TOO_LARGE = "it is 600 MB, and Writ opens files up to 524 MB.";

beforeEach(() => {
  vi.clearAllMocks();
});

describe("reportOpenOutcomes", () => {
  it("names the file and the reason of a single refusal", () => {
    reportOpenOutcomes([{ kind: "refused", path: "/logs/dump.log", reason: TOO_LARGE }]);

    expect(toast).toHaveBeenCalledTimes(1);
    expect(toast).toHaveBeenCalledWith(`Couldn't open dump.log: ${TOO_LARGE}`, "error");
  });

  it("names a Windows file by its own name", () => {
    reportOpenOutcomes([
      { kind: "refused", path: "C:\\Users\\me\\logs\\dump.log", reason: TOO_LARGE },
    ]);

    expect(toast).toHaveBeenCalledWith(`Couldn't open dump.log: ${TOO_LARGE}`, "error");
  });

  it("says nothing about Cancel, an open, or a file still downloading", () => {
    reportOpenOutcomes([
      { kind: "declined", path: "/logs/big.log" },
      { kind: "opened", doc: row("a", "/a.txt"), existed: false, mode: { kind: "Normal" } },
      { kind: "not-downloaded", path: "/cloud/away.md", provider: null },
    ]);

    expect(toast).not.toHaveBeenCalled();
  });

  it("collapses the refusals of one gesture into one line", () => {
    reportOpenOutcomes([
      { kind: "refused", path: "/a.log", reason: TOO_LARGE },
      { kind: "opened", doc: row("b", "/b.txt"), existed: false, mode: { kind: "Normal" } },
      { kind: "refused", path: "/c.log", reason: "you do not have permission to read it." },
    ]);

    expect(toast).toHaveBeenCalledTimes(1);
    expect(toast).toHaveBeenCalledWith("Couldn't open 2 files", "error");
  });
});

describe("the tab store's open", () => {
  function storeAnswering(outcome: OpenOutcome) {
    const registry = { openFile: vi.fn(async () => outcome) } as unknown as BufferRegistry;
    const downloads = {
      select: vi.fn(),
      start: vi.fn(async () => undefined),
    } as unknown as DownloadStore;
    return createTabStore({ registry, downloads });
  }

  it("reports a refusal once and opens no tab", async () => {
    const tabs = storeAnswering({ kind: "refused", path: "/logs/dump.log", reason: TOO_LARGE });

    expect(await tabs.openFile("/logs/dump.log")).toBeNull();

    expect(tabs.activeTabId()).toBeNull();
    expect(toast).toHaveBeenCalledTimes(1);
    expect(toast).toHaveBeenCalledWith(`Couldn't open dump.log: ${TOO_LARGE}`, "error");
  });

  it("treats Cancel as an answer: no tab, no message, no write of the tab in front", async () => {
    const tabs = storeAnswering({ kind: "declined", path: "/logs/big.log" });

    await expect(tabs.openFile("/logs/big.log")).resolves.toBeNull();

    expect(toast).not.toHaveBeenCalled();
    expect(flushAutosave).not.toHaveBeenCalled();
    expect(tabs.activeTabId()).toBeNull();
  });

  it("hands the answer to a caller that reports for itself, without a message", async () => {
    const outcome: OpenOutcome = { kind: "refused", path: "/a.log", reason: TOO_LARGE };
    const tabs = storeAnswering(outcome);

    expect(await tabs.openPath("/a.log")).toEqual(outcome);
    expect(toast).not.toHaveBeenCalled();
  });

  it("ends the progress of a file opened behind the tab in front", async () => {
    const doc = row("big", "/logs/big.log");
    const tabs = storeAnswering({ kind: "opened", doc, existed: false, mode: { kind: "LargeFile" } });

    await tabs.openFile("/logs/big.log", { activate: false });

    expect(largeFileOpen.finishFor).toHaveBeenCalledWith("big");
    expect(tabs.activeTabId()).toBeNull();
  });

  it("selects a file it opened in front and leaves the progress to the editor", async () => {
    const doc = row("big", "/logs/big.log");
    const tabs = storeAnswering({ kind: "opened", doc, existed: false, mode: { kind: "LargeFile" } });

    expect(await tabs.openFile("/logs/big.log")).toEqual(doc);

    expect(tabs.activeTabId()).toBe("big");
    expect(largeFileOpen.finishFor).not.toHaveBeenCalled();
  });
});
