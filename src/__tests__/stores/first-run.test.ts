import { describe, it, expect, vi, beforeEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
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
    expect(mocks.finishFirstRun).not.toHaveBeenCalled();

    mocks.finishFirstRun.mockResolvedValue(DOC);
    await first.continueSetup();

    expect(mocks.finishFirstRun).toHaveBeenCalledWith("md");
    expect(configStore.config().files.default_extension).toBe("md");
    expect(bufferRegistry.activeTabs().map((b) => b.id)).toContain(DOC.id);
    expect(first.step()).toBeNull();
  });

  // A launch with tabs to restore opens no new note; the screen still leaves.
  it("leaves the screen when there is no note to open", async () => {
    const first = store();
    await first.load();
    mocks.finishFirstRun.mockResolvedValue(null);

    await first.continueSetup();

    expect(first.step()).toBeNull();
    expect(bufferRegistry.activeTabs().map((b) => b.id)).not.toContain("note-none");
  });

  it("keeps the question up when the answer could not be recorded", async () => {
    const first = store();
    await first.load();
    first.setFormat("md");
    mocks.finishFirstRun.mockRejectedValue(new Error("no IPC"));

    await first.continueSetup();

    expect(first.step()).toBe("format");
    expect(first.format()).toBe("md");
  });
});

// The boot runs once, in App's onMount, so no mount test can watch its order.
describe("where the boot reads the answer", () => {
  const source = readFileSync(resolve(process.cwd(), "src/App.tsx"), "utf8");

  it("reads it before the window is shaped, not after it is shown", () => {
    const read = source.indexOf("await firstRunStore.load();");
    const firstTab = source.indexOf("await win.tabs.createTab();");
    expect(read).toBeGreaterThan(-1);
    expect(firstTab).toBeGreaterThan(read);
  });

  it("mints no note behind the question", () => {
    expect(source).toMatch(
      /else if \(firstRunStore\.step\(\) === null\) \{[\s\S]*?await win\.tabs\.createTab\(\);/,
    );
  });
});
