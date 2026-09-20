import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import { Show } from "solid-js";
import WindowProvider, { useWindow } from "../../components/WindowProvider/WindowProvider";
import type { WindowState } from "../../stores/window/createWindowState";
import type { BufferDocument } from "../../types/buffer";

const mocks = vi.hoisted(() => ({
  firstRunState: vi.fn(),
  finishFirstRun: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  firstRunState: mocks.firstRunState,
  finishFirstRun: mocks.finishFirstRun,
  dismissFirstRunHint: vi.fn().mockResolvedValue(undefined),
  autoRetitleNote: vi.fn().mockResolvedValue({ kind: "skipped" }),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  getBuffer: vi.fn(),
  renameNote: vi.fn(),
  renameNoteWithLinks: vi.fn(),
  previewClose: vi.fn().mockResolvedValue(undefined),
  previewListRenderers: vi.fn().mockResolvedValue([]),
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

import FirstRunSetup from "../../components/FirstRun/FirstRunSetup";
import { firstRunStore } from "../../stores/global/first-run";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { configStore } from "../../stores/global/config";

const DOC = {
  id: "note-first",
  title: "Untitled.md",
  filename: "Untitled.md",
  status: "active",
  source_path: "/notes/Untitled.md",
} as unknown as BufferDocument;

let win: WindowState | null = null;

function CaptureWindow() {
  win = useWindow();
  return null;
}

function mount() {
  return render(() => (
    <WindowProvider windowId={8802}>
      <CaptureWindow />
      <Show when={firstRunStore.step() !== null}>
        <FirstRunSetup />
      </Show>
    </WindowProvider>
  ));
}

function option(container: Element, format: string): HTMLElement {
  const el = container.querySelector<HTMLElement>(`[data-format='${format}']`);
  if (!el) throw new Error(`no ${format} option`);
  return el;
}

describe("the format the first launch asks about", () => {
  beforeEach(() => {
    mocks.firstRunState.mockReset().mockResolvedValue({
      first_run: true,
      hint_dismissed: false,
      file_manager: "Finder",
    });
    mocks.finishFirstRun.mockReset().mockResolvedValue(null);
    win = null;
  });

  afterEach(() => cleanup());

  it("stays off the screen while the launch has nothing to ask", () => {
    const step = vi.spyOn(firstRunStore, "step").mockReturnValue(null);
    const { container } = mount();
    expect(container.querySelector(".first-run-setup")).toBeNull();
    step.mockRestore();
  });

  it("offers the two formats with plain text already chosen", async () => {
    await firstRunStore.load();
    const { container } = mount();

    await waitFor(() => expect(container.querySelector(".first-run-setup")).not.toBeNull());
    expect(option(container, "txt").textContent).toBe(
      "Plain text (.txt)No markup. Opens in any editor.",
    );
    expect(option(container, "md").textContent).toBe(
      "Markdown (.md)Headings, lists and links, with a rendered view.",
    );
    expect(option(container, "txt").getAttribute("aria-checked")).toBe("true");
    expect(option(container, "md").getAttribute("aria-checked")).toBe("false");
  });

  it("moves between the two options with the arrow keys", async () => {
    await firstRunStore.load();
    const { container } = mount();

    fireEvent.keyDown(option(container, "txt"), { key: "ArrowRight" });
    await waitFor(() => expect(option(container, "md").getAttribute("aria-checked")).toBe("true"));
    expect(option(container, "md").getAttribute("tabindex")).toBe("0");
    expect(option(container, "txt").getAttribute("tabindex")).toBe("-1");

    fireEvent.keyDown(option(container, "md"), { key: "ArrowLeft" });
    await waitFor(() => expect(option(container, "txt").getAttribute("aria-checked")).toBe("true"));
  });

  // The screen is the only place the answer exists until Continue lands, so a
  // failed call leaves it up rather than dropping the question.
  it("keeps the screen up when the answer could not be recorded", async () => {
    await firstRunStore.load();
    mocks.finishFirstRun.mockRejectedValue(new Error("no IPC"));
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const { container } = mount();

    fireEvent.click(option(container, "md"));
    fireEvent.click(container.querySelector("button")!);

    await waitFor(() => expect(mocks.finishFirstRun).toHaveBeenCalledWith("md"));
    expect(container.querySelector(".first-run-setup")).not.toBeNull();
    consoleSpy.mockRestore();
  });

  it("records the chosen format, opens the note it answers with, and leaves", async () => {
    await firstRunStore.load();
    mocks.finishFirstRun.mockResolvedValue(DOC);
    const { container } = mount();

    fireEvent.click(option(container, "md"));
    fireEvent.click(container.querySelector("button")!);

    await waitFor(() => expect(container.querySelector(".first-run-setup")).toBeNull());
    expect(mocks.finishFirstRun).toHaveBeenCalledWith("md");
    expect(configStore.config().files.default_extension).toBe("md");
    expect(bufferRegistry.activeTabs().map((b) => b.id)).toContain(DOC.id);
    expect(win!.tabs.activeTabId()).toBe(DOC.id);
  });
});
