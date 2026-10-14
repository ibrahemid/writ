import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import type { ContextPack } from "../../services/tauri";

const h = vi.hoisted(() => ({
  promptContextPack: vi.fn(),
  writeClipboardText: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  promptContextPack: h.promptContextPack,
  noteAllTags: vi.fn().mockResolvedValue([{ tag: "launch", count: 2 }]),
  getNotesRoot: vi.fn().mockResolvedValue("/Users/me/Writ"),
  writeClipboardText: h.writeClipboardText,
  readClipboardText: vi.fn(),
}));
vi.mock("../../services/autosave", () => ({ collectUnsavedContent: () => [] }));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    activeTabs: () => [{ id: "plan", source_path: "/Users/me/Writ/work/Plan.md" }],
  },
}));
vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => ({ tabs: { activeTabId: () => "plan" } }) },
}));
vi.mock("../../stores/global/notifications", () => ({ showToast: vi.fn() }));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: vi.fn() } }),
}));

import ContextPackModal from "../../components/ContextPack/ContextPackModal";
import { contextPackStore } from "../../stores/global/context-pack";

const PACK: ContextPack = {
  text: '<file path="work/Plan.md">\nSee [[Budget]].\n</file>\n\n<file path="work/Budget.md">\nTen.\n</file>',
  tokens: 1500,
  files: [
    { path: "work/Plan.md", tokens: 1200 },
    { path: "work/Budget.md", tokens: 300 },
  ],
  skipped: [{ path: "work/photo.png", reason: "not_text" }],
  omitted: 3,
};

function text(selector: string): string {
  return document.querySelector(selector)?.textContent ?? "";
}

beforeEach(() => {
  h.promptContextPack.mockReset().mockResolvedValue(PACK);
  h.writeClipboardText.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  contextPackStore.close();
  cleanup();
});

describe("Copy files as prompt", () => {
  it("shows the count, the files, what was left out and the text before copying", async () => {
    render(() => <ContextPackModal />);
    await contextPackStore.open();

    expect(document.querySelector("[role='dialog']")?.getAttribute("aria-labelledby")).toBe(
      "context-pack-title",
    );
    expect(text(".context-pack-meta")).toBe("2 files · ~1.5k tokens");
    expect(
      Array.from(document.querySelectorAll(".context-pack-path")).map((el) => el.textContent),
    ).toEqual(["work/Plan.md", "work/Budget.md"]);
    expect(text(".context-pack-body")).toContain("Not included: work/photo.png (not text)");
    expect(text(".context-pack-body")).toContain("3 more files were left out");
    expect(text(".context-pack-preview")).toBe(PACK.text);
  });

  it("offers the file's links, its folder and a tag", async () => {
    render(() => <ContextPackModal />);
    await contextPackStore.open();
    await waitFor(() => expect(document.querySelectorAll("option")).toHaveLength(1));

    const labels = Array.from(document.querySelectorAll(".context-pack-option")).map(
      (el) => el.textContent,
    );
    expect(labels).toEqual([
      "This file and the files it links to",
      "Files in work",
      "Files tagged",
    ]);
    const checked = document.querySelector<HTMLInputElement>("input[type='radio']:checked");
    expect(checked?.value).toBe("linked");
  });

  it("packs the folder when its option is chosen", async () => {
    render(() => <ContextPackModal />);
    await contextPackStore.open();

    fireEvent.click(document.querySelector<HTMLInputElement>("input[value='folder']")!);
    await waitFor(() => expect(h.promptContextPack).toHaveBeenCalledTimes(2));
    expect(h.promptContextPack.mock.calls[1][0]).toEqual({
      kind: "folder",
      path: "/Users/me/Writ/work",
    });
  });

  it("copies the whole prompt and closes", async () => {
    render(() => <ContextPackModal />);
    await contextPackStore.open();

    fireEvent.click(document.querySelector<HTMLButtonElement>(".context-pack-copy")!);
    await waitFor(() => expect(h.writeClipboardText).toHaveBeenCalledWith(PACK.text));
    await waitFor(() => expect(document.querySelector("[role='dialog']")).toBeNull());
  });

  it("closes on Escape without copying", async () => {
    render(() => <ContextPackModal />);
    await contextPackStore.open();

    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    await waitFor(() => expect(document.querySelector("[role='dialog']")).toBeNull());
    expect(h.writeClipboardText).not.toHaveBeenCalled();
  });

  it("cannot copy while nothing was read", async () => {
    h.promptContextPack.mockRejectedValue("no index");
    render(() => <ContextPackModal />);
    await contextPackStore.open();

    expect(text(".context-pack-meta")).toBe("Could not read the files.");
    expect(document.querySelector<HTMLButtonElement>(".context-pack-copy")!.disabled).toBe(true);
  });
});
