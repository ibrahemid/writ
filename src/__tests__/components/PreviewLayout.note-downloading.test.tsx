import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { windowRegistry } from "../../stores/global/window-registry";

// The wiring test for the download pane: the editor slot is where it shows,
// and it shows only when there is no buffer to render instead. Every other
// test in the set renders NoteDownloading with props of its own, so this is
// the one that would catch the pane being mounted where nothing reaches it.

const mocks = vi.hoisted(() => ({
  materialiseNote: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../services/tauri", () => ({
  previewListRenderers: vi.fn().mockResolvedValue([]),
  previewForceRender: vi.fn().mockResolvedValue(null),
  previewRender: vi.fn().mockResolvedValue(null),
  previewClose: vi.fn().mockResolvedValue(undefined),
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
  searchBuffers: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
  materialiseNote: mocks.materialiseNote,
  cancelMaterialiseNote: vi.fn().mockResolvedValue(undefined),
}));

import PreviewLayout from "../../components/Preview/PreviewLayout";

describe("PreviewLayout — the editor slot hosts a note that is downloading", () => {
  afterEach(() => cleanup());

  it("shows the download state in place of the editor, and the empty pane without one", async () => {
    const { container } = render(() => (
      <WindowProvider windowId={7401}>
        <PreviewLayout buffer={null} />
      </WindowProvider>
    ));

    // No buffer and no download: the ordinary empty editor pane.
    expect(container.querySelector(".note-downloading")).toBeNull();

    const win = windowRegistry.getActive()!;
    await win.downloads.start({
      path: "/home/user/Writ/trip.md",
      title: "trip.md",
      provider: "iCloud Drive",
    });

    await waitFor(() =>
      expect(container.querySelector(".note-downloading")).not.toBeNull(),
    );
    expect(container.textContent).toContain("Downloading from iCloud Drive");
    expect(mocks.materialiseNote).toHaveBeenCalledWith("/home/user/Writ/trip.md");

    // Closing the download takes the pane with it.
    win.downloads.close("/home/user/Writ/trip.md");
    await waitFor(() =>
      expect(container.querySelector(".note-downloading")).toBeNull(),
    );
  });
});
