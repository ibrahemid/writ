import { describe, it, expect, afterEach, vi } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import type { BufferDocument, FileOpenMode } from "../../types/buffer";
import type { MenuItem } from "../../components/ContextMenu/ContextMenu";

// The tier chip says what the file in front has turned off and why, and holds
// the per-file choices for wrapping and highlighting (EP-10).

const [mode, setMode] = createSignal<FileOpenMode | null>(null);
const [offer, setOffer] = createSignal<{ bufferId: string; mode: FileOpenMode } | null>(null);
const setOverride = vi.fn();
const acceptTierOffer = vi.fn();
const showAnchoredMenu = vi.hoisted(() => vi.fn());

vi.mock("../../stores/global/save-status", () => ({
  saveStatusStore: { forNote: () => ({ state: "clean" as const, fileName: "export.md" }) },
}));
vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({ editor: { status_bar_counts: false }, spelling: { enabled: true } }),
  },
}));
vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => null },
}));
vi.mock("../../stores/global/renderer-registry", () => ({
  rendererRegistry: { hasRenderer: () => false },
}));
vi.mock("../../lib/use-active-buffer", () => ({
  useActiveBuffer: () => () =>
    ({ id: "tab-1", title: "export.md", filename: "export.md", source_path: "/w/export.md", encoding: "UTF-8" }) as BufferDocument,
}));
vi.mock("../../components/ContextMenu/ContextMenu", () => ({
  showAnchoredMenu,
  isMenuOpenFor: () => false,
}));
vi.mock("../../components/Preview/PreviewLayoutToggle", () => ({ default: () => null }));
vi.mock("../../components/Preview/PreviewScriptsField", () => ({ default: () => null }));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: {
      largeFileMode: mode,
      currentBufferId: () => "tab-1",
      tierOffer: offer,
      longestLine: () => 0,
      overridesOf: () => ({}),
      setOverride,
      acceptTierOffer,
      cursorLine: () => 1,
      cursorCol: () => 1,
      isUpdatedFromDisk: () => false,
    },
    counts: { counts: () => null, watch: () => () => {} },
    tabs: { activeTabId: () => "tab-1" },
  }),
}));

import StatusBar from "../../components/Editor/StatusBar";

function chip(container: HTMLElement): HTMLButtonElement | null {
  return container.querySelector<HTMLButtonElement>(".statusbar-chip--largefile");
}

function openedItems(): MenuItem[] {
  return showAnchoredMenu.mock.calls[showAnchoredMenu.mock.calls.length - 1][1] as MenuItem[];
}

afterEach(() => {
  setMode(null);
  setOffer(null);
  vi.clearAllMocks();
  cleanup();
});

describe("StatusBar tier chip", () => {
  it("shows nothing for a normal file", () => {
    const { container } = render(() => <StatusBar />);
    expect(chip(container)).toBeNull();
  });

  it("opens what a large file turns off, with wrapping and highlighting to turn on", () => {
    setMode({ kind: "LargeFile" });
    const { container } = render(() => <StatusBar />);
    const button = chip(container)!;
    expect(button.textContent).toBe("Large file");
    expect(button.getAttribute("aria-haspopup")).toBe("menu");

    button.click();

    const items = openedItems();
    expect(items.filter((item) => item.note).map((item) => item.label)).toEqual([
      "Files over 5 MB open in large-file mode.",
      "Off: Markdown formatting, spell check, links and text direction.",
      "Not in search results.",
    ]);
    const wrap = items.find((item) => item.label === "Wrap lines")!;
    expect(wrap.checked).toBe(false);
    expect(items.find((item) => item.label === "Highlight syntax")?.checked).toBe(false);

    wrap.action();
    expect(setOverride).toHaveBeenCalledWith("tab-1", "wrap", true);
  });

  it("offers large-file mode for a note a paste took past 5 MB", () => {
    setOffer({ bufferId: "tab-1", mode: { kind: "LargeFile" } });
    const { container } = render(() => <StatusBar />);
    const button = chip(container)!;
    expect(button.textContent).toBe("Over 5 MB");

    button.click();
    openedItems().find((item) => item.label === "Use large-file mode")!.action();

    expect(acceptTierOffer).toHaveBeenCalledTimes(1);
  });
});
