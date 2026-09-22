import { describe, it, expect, vi, beforeEach } from "vitest";
import { createRoot } from "solid-js";
import type { BufferDocument } from "../../types/buffer";

const mocks = vi.hoisted(() => ({
  firstRunState: vi.fn(),
  finishFirstRun: vi.fn(),
  dismissFirstRunHint: vi.fn(),
  autoRetitleNote: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  firstRunState: mocks.firstRunState,
  finishFirstRun: mocks.finishFirstRun,
  dismissFirstRunHint: mocks.dismissFirstRunHint,
  autoRetitleNote: mocks.autoRetitleNote,
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  renameNote: vi.fn(),
  renameNoteWithLinks: vi.fn(),
  getBuffer: vi.fn(),
  previewClose: vi.fn().mockResolvedValue(undefined),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

import { createFirstRunStore } from "../../stores/global/first-run";
import { openBootTab } from "../../stores/window/boot-tab";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { configStore } from "../../stores/global/config";

const DOC = {
  id: "note-first",
  title: "Untitled.md",
  filename: "Untitled.md",
  status: "active",
  source_path: "/notes/Untitled.md",
} as unknown as BufferDocument;

function store() {
  return createRoot(createFirstRunStore);
}

describe("what the first launch asks", () => {
  beforeEach(() => {
    mocks.firstRunState.mockReset().mockResolvedValue({
      first_run: true,
      hint_dismissed: false,
      file_manager: "Finder",
    });
    mocks.finishFirstRun.mockReset().mockResolvedValue(null);
  });

  it("asks about the format on a first launch and about nothing on any other", async () => {
    const first = store();
    await first.load();
    expect(first.step()).toBe("format");
    expect(first.format()).toBe("txt");

    mocks.firstRunState.mockResolvedValue({
      first_run: false,
      hint_dismissed: false,
      file_manager: "Finder",
    });
    const later = store();
    await later.load();
    expect(later.step()).toBeNull();
  });

  it("writes nothing until Continue, and then writes the answer that is on screen", async () => {
    const first = store();
    await first.load();
    first.setFormat("md");
    first.continueFormat();
    expect(first.step()).toBe("apps");
    expect(mocks.finishFirstRun).not.toHaveBeenCalled();

    mocks.finishFirstRun.mockResolvedValue(DOC);
    await first.continueApps();

    expect(mocks.finishFirstRun).toHaveBeenCalledWith("md", []);
    expect(configStore.config().files.default_extension).toBe("md");
    expect(bufferRegistry.activeTabs().map((b) => b.id)).toContain(DOC.id);
    expect(first.step()).toBeNull();
  });

  // A launch with tabs to restore opens no new note; the screen still leaves.
  it("leaves the screen when there is no note to open", async () => {
    const first = store();
    await first.load();
    mocks.finishFirstRun.mockResolvedValue(null);

    first.continueFormat();
    await first.continueApps();

    expect(first.step()).toBeNull();
    expect(bufferRegistry.activeTabs().map((b) => b.id)).not.toContain("note-none");
  });

  it("keeps the question up when the answer could not be recorded", async () => {
    const first = store();
    await first.load();
    first.setFormat("md");
    first.continueFormat();
    first.toggleApp("graph");
    mocks.finishFirstRun.mockRejectedValue(new Error("no IPC"));

    await first.continueApps();

    expect(first.step()).toBe("apps");
    expect(first.format()).toBe("md");
    expect(first.isAppChosen("graph")).toBe(true);
  });
});

describe("the tab the window opens on", () => {
  function tabs(activeTabId: string | null) {
    return {
      activeTabId: vi.fn(() => activeTabId),
      setActiveTabId: vi.fn(),
      createTab: vi.fn().mockResolvedValue(undefined),
    };
  }

  it("restores the last tab the session left", async () => {
    const actions = tabs(null);

    await openBootTab(actions, [{ id: "older" }, { id: "newest" }] as BufferDocument[], null);

    expect(actions.setActiveTabId).toHaveBeenCalledWith("newest");
    expect(actions.createTab).not.toHaveBeenCalled();
  });

  it("creates one when there is nothing to restore", async () => {
    const actions = tabs(null);

    await openBootTab(actions, [], null);

    expect(actions.createTab).toHaveBeenCalledTimes(1);
  });

  // The question is answered on an empty window, and the note the answer
  // carries is the one that opens. A tab minted here would be a second empty
  // note nobody asked for.
  it("mints nothing while the first launch is still asking", async () => {
    const actions = tabs(null);

    await openBootTab(actions, [], "format");

    expect(actions.createTab).not.toHaveBeenCalled();
    expect(actions.setActiveTabId).not.toHaveBeenCalled();
  });

  it("leaves a window that already has a tab alone", async () => {
    const actions = tabs("open");

    await openBootTab(actions, [{ id: "newest" }] as BufferDocument[], null);

    expect(actions.createTab).not.toHaveBeenCalled();
    expect(actions.setActiveTabId).not.toHaveBeenCalled();
  });
});
