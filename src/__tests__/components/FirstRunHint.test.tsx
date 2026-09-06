import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import { createRoot } from "solid-js";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
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
} as unknown as WritConfig;

function mount() {
  return render(() => (
    <WindowProvider windowId={8801}>
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
});
