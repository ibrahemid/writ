import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { BufferDocument } from "../../types/buffer";

// The layout keymap per content type: the cycle alternates on a Markdown
// file, and the split commands are no-ops there because it has no split.

const mocks = vi.hoisted(() => ({
  activeTabs: vi.fn<() => BufferDocument[]>(() => []),
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: mocks.activeTabs },
}));

vi.mock("../../services/tauri", () => ({
  previewGetLayout: mocks.previewGetLayout,
  previewSetLayout: mocks.previewSetLayout,
}));

import { getCommand, unregisterCommand } from "../../commands/registry";
import { windowRegistry } from "../../stores/global/window-registry";
import { createWindowState } from "../../stores/window/createWindowState";
import { registerPreviewKeymap } from "../../keymap/preview";
import { defaultSplit } from "../../lib/preview-layout";

const COMMAND_IDS = [
  "preview.cycleLayout",
  "preview.refresh",
  "preview.toggleFullscreen",
  "preview.exitFullscreen",
  "preview.swapOrientation",
  "preview.resetRatio",
  "preview.toggleRunScripts",
];

function buffer(id: string, name: string): BufferDocument {
  return {
    id,
    title: name,
    filename: name,
    status: "active",
    language: null,
    source_path: `/files/${name}`,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "",
    updated_at: "",
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
  };
}

function run(id: string) {
  getCommand(id)!.execute();
}

describe("preview layout keymap", () => {
  let windowId = 0;
  let release: (() => void) | null = null;

  beforeEach(() => {
    windowId += 1;
    release = windowRegistry.register(createWindowState({ windowId }));
    windowRegistry.focus(windowId);
    registerPreviewKeymap();
  });

  afterEach(() => {
    for (const id of COMMAND_IDS) unregisterCommand(id);
    release?.();
    release = null;
    mocks.activeTabs.mockReturnValue([]);
  });

  function activate(buf: BufferDocument) {
    mocks.activeTabs.mockReturnValue([buf]);
    const win = windowRegistry.getActive()!;
    win.tabs.setActiveTabId(buf.id);
    return win;
  }

  it("cycles a markdown buffer between inline and source", () => {
    const win = activate(buffer("K1", "plan.md"));
    win.layout.setLocal("K1", { kind: "inline" });

    run("preview.cycleLayout");
    expect(win.layout.get("K1", "markdown")).toEqual({ kind: "source" });

    run("preview.cycleLayout");
    expect(win.layout.get("K1", "markdown")).toEqual({ kind: "inline" });
  });

  it("leaves the html cycle unchanged", () => {
    const win = activate(buffer("K2", "page.html"));
    win.layout.setLocal("K2", { kind: "source" });

    run("preview.cycleLayout");
    expect(win.layout.get("K2", "html").kind).toBe("split");

    run("preview.cycleLayout");
    expect(win.layout.get("K2", "html")).toEqual({ kind: "preview" });

    run("preview.cycleLayout");
    expect(win.layout.get("K2", "html")).toEqual({ kind: "source" });
  });

  it("swap orientation is a no-op on an inline markdown buffer", () => {
    const win = activate(buffer("K3", "plan.md"));
    win.layout.setLocal("K3", { kind: "inline" });

    run("preview.swapOrientation");
    expect(win.layout.get("K3", "markdown")).toEqual({ kind: "inline" });
  });

  it("swap orientation still flips an html split", () => {
    const win = activate(buffer("K4", "page.html"));
    win.layout.setLocal("K4", defaultSplit());

    run("preview.swapOrientation");
    const flipped = win.layout.get("K4", "html");
    expect(flipped.kind === "split" && flipped.orientation).toBe("horizontal");
  });

  it("fullscreen, escape and reset ratio are no-ops on a markdown buffer", () => {
    const win = activate(buffer("K5", "plan.md"));
    win.layout.setLocal("K5", { kind: "inline" });

    run("preview.toggleFullscreen");
    expect(win.layout.get("K5", "markdown")).toEqual({ kind: "inline" });

    run("preview.exitFullscreen");
    expect(win.layout.get("K5", "markdown")).toEqual({ kind: "inline" });

    run("preview.resetRatio");
    expect(win.layout.get("K5", "markdown")).toEqual({ kind: "inline" });
  });

  it("escape declines when there is no fullscreen preview to leave, so the key reaches the editor", () => {
    const exit = () => getCommand("preview.exitFullscreen")!.execute();
    expect(exit()).toBe(false);

    const markdown = activate(buffer("K7", "plan.md"));
    markdown.layout.setLocal("K7", { kind: "inline" });
    expect(exit()).toBe(false);

    const html = activate(buffer("K8", "page.html"));
    html.layout.setLocal("K8", defaultSplit());
    expect(exit()).toBe(false);

    run("preview.toggleFullscreen");
    expect(html.layout.get("K8", "html")).toEqual({ kind: "preview" });
    expect(exit()).not.toBe(false);
    expect(html.layout.get("K8", "html")).toEqual(defaultSplit());
    expect(exit()).toBe(false);
  });

  it("reset ratio still re-derives an html split", () => {
    const win = activate(buffer("K6", "page.html"));
    win.layout.setLocal("K6", { kind: "source" });

    run("preview.resetRatio");
    expect(win.layout.get("K6", "html")).toEqual(defaultSplit());
  });
});
