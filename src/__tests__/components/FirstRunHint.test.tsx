import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import { createRoot } from "solid-js";
import WindowProvider, { useWindow } from "../../components/WindowProvider/WindowProvider";
import type { WindowState } from "../../stores/window/createWindowState";
import { configStore } from "../../stores/global/config";
import type { WritConfig } from "../../types/config";

const mocks = vi.hoisted(() => ({
  firstRunState: vi.fn(),
  dismissFirstRunHint: vi.fn(),
  autoRetitleNote: vi.fn(),
  config: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  firstRunState: mocks.firstRunState,
  dismissFirstRunHint: mocks.dismissFirstRunHint,
  autoRetitleNote: mocks.autoRetitleNote,
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  getBuffer: vi.fn(),
  renameNote: vi.fn(),
  previewListRenderers: vi.fn().mockResolvedValue([]),
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

vi.spyOn(configStore, "config").mockImplementation(() => mocks.config() as WritConfig);

import FirstRunHint from "../../components/Editor/FirstRunHint";
import {
  createFirstRunStore,
  firstRunStore,
  hintText,
  offerText,
} from "../../stores/global/first-run";

const CONFIG = {
  sidebar: { open: false, width: 240, position: "left" },
  editor: { status_bar: false },
  first_run: { hint_dismissed: false },
} as unknown as WritConfig;

let win: WindowState | null = null;

function CaptureWindow() {
  win = useWindow();
  return null;
}

function mount() {
  return render(() => (
    <WindowProvider windowId={8801}>
      <CaptureWindow />
      <FirstRunHint />
    </WindowProvider>
  ));
}

describe("the first launch's one line", () => {
  beforeEach(() => {
    mocks.config.mockReset().mockReturnValue(CONFIG);
    mocks.dismissFirstRunHint.mockReset().mockResolvedValue(undefined);
    mocks.autoRetitleNote.mockReset().mockResolvedValue({ kind: "skipped" });
    mocks.firstRunState.mockReset().mockResolvedValue({
      first_run: true,
      hint_dismissed: false,
      file_manager: "Finder",
    });
    firstRunStore.dismissOffer();
    win = null;
  });

  afterEach(() => cleanup());

  it("names the platform's own file manager, whichever platform this is", () => {
    expect(hintText("Finder")).toBe(
      "Your notes are saved automatically to a folder you can open in Finder.",
    );
    expect(hintText("File Explorer")).toBe(
      "Your notes are saved automatically to a folder you can open in File Explorer.",
    );
    expect(hintText("Files")).toBe(
      "Your notes are saved automatically to a folder you can open in Files.",
    );
  });

  it("shows the line once, takes it away on the first keystroke, and does not bring it back", async () => {
    await firstRunStore.load();
    const first = mount();
    await waitFor(() =>
      expect(first.container.querySelector(".first-run-hint")?.textContent).toBe(
        hintText("Finder"),
      ),
    );

    firstRunStore.dismissHint();
    await waitFor(() => expect(first.container.querySelector(".first-run-hint")).toBeNull());
    expect(mocks.dismissFirstRunHint).toHaveBeenCalledTimes(1);

    // Remounting is what a window that reloads does; the line stays gone.
    cleanup();
    const second = mount();
    await waitFor(() => expect(second.container).not.toBeNull());
    expect(second.container.querySelector(".first-run-hint")).toBeNull();
  });

  it("says nothing on a launch that is not the first", async () => {
    // A second launch is a second store: the singleton reads once, and the
    // read it did is the one the tests above exercised.
    mocks.firstRunState.mockResolvedValue({
      first_run: false,
      hint_dismissed: false,
      file_manager: "Finder",
    });
    const later = createRoot(createFirstRunStore);
    await later.load();
    expect(later.showHint()).toBe(false);

    // Nor on a first launch whose line has already been dismissed.
    mocks.firstRunState.mockResolvedValue({
      first_run: true,
      hint_dismissed: true,
      file_manager: "Finder",
    });
    const dismissed = createRoot(createFirstRunStore);
    await dismissed.load();
    expect(dismissed.showHint()).toBe(false);
  });

  it("asks before renaming a note something else has touched", async () => {
    mocks.autoRetitleNote.mockResolvedValue({ kind: "ask", title: "Grocery list" });
    const { container } = mount();
    win!.tabs.setActiveTabId("note-1");

    await firstRunStore.offerRetitle("note-1");
    await waitFor(() =>
      expect(container.querySelector(".first-run-offer-text")?.textContent).toBe(
        offerText("Grocery list"),
      ),
    );

    const keep = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent === "Keep the date",
    );
    fireEvent.click(keep!);
    await waitFor(() => expect(container.querySelector(".first-run-offer")).toBeNull());
  });

  it("asks once per note", async () => {
    mocks.autoRetitleNote.mockResolvedValue({ kind: "ask", title: "Grocery list" });
    mount();
    await firstRunStore.offerRetitle("note-2");
    await firstRunStore.offerRetitle("note-2");
    expect(mocks.autoRetitleNote).toHaveBeenCalledTimes(1);
  });

  it("asks again about an empty note, and only once about every other note", async () => {
    mocks.autoRetitleNote.mockResolvedValue({ kind: "not_yet" });
    mount();
    await firstRunStore.offerRetitle("note-3");
    await firstRunStore.offerRetitle("note-3");
    expect(mocks.autoRetitleNote).toHaveBeenCalledTimes(2);

    mocks.autoRetitleNote.mockResolvedValue({ kind: "skipped" });
    await firstRunStore.offerRetitle("note-4");
    await firstRunStore.offerRetitle("note-4");
    expect(mocks.autoRetitleNote).toHaveBeenCalledTimes(3);
  });

  it("shows the offer over the note it names, not over the one moved on to", async () => {
    mocks.autoRetitleNote.mockResolvedValue({ kind: "ask", title: "Grocery list" });
    const { container } = mount();
    win!.tabs.setActiveTabId("note-6");

    await firstRunStore.offerRetitle("note-5");
    await waitFor(() => expect(container.querySelector(".first-run-offer")).toBeNull());

    win!.tabs.setActiveTabId("note-5");
    await waitFor(() =>
      expect(container.querySelector(".first-run-offer-text")?.textContent).toBe(
        offerText("Grocery list"),
      ),
    );
  });

  it("stands on the status bar rather than over it", async () => {
    await firstRunStore.load();
    const off = mount();
    await waitFor(() =>
      expect(off.container.querySelector<HTMLElement>(".first-run-layer")?.style.bottom).toBe("0px"),
    );

    cleanup();
    mocks.config.mockReturnValue({
      ...CONFIG,
      editor: { status_bar: true },
    } as unknown as WritConfig);
    const on = mount();
    await waitFor(() =>
      expect(on.container.querySelector<HTMLElement>(".first-run-layer")?.style.bottom).toBe(
        "var(--writ-statusbar-height)",
      ),
    );
  });
});
