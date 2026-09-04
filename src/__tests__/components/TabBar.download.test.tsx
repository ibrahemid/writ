import { describe, it, expect, vi, beforeAll, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import { createDownloadStore } from "../../stores/window/download-store";

beforeAll(() => {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
  if (!Element.prototype.scrollIntoView) {
    Element.prototype.scrollIntoView = vi.fn();
  }
});

const mocks = vi.hoisted(() => ({
  materialiseNote: vi.fn().mockResolvedValue(undefined),
  cancelMaterialiseNote: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../services/tauri", () => ({
  materialiseNote: mocks.materialiseNote,
  cancelMaterialiseNote: mocks.cancelMaterialiseNote,
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn().mockResolvedValue(() => undefined),
}));

// The real download store, so the tab's own control is followed all the way to
// the command that stops the wait.
const downloads = createDownloadStore();

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    tabs: {
      activeTabId: () => null,
      setActiveTabId: vi.fn(),
      closeTab: vi.fn(),
      closeOtherTabs: vi.fn(),
      closeAllTabs: vi.fn(),
      createTab: vi.fn(),
    },
    downloads,
  }),
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    activeTabs: () => [],
    renameBuffer: vi.fn(),
  },
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({ tabs: { activeTabId: () => null } }),
  },
}));

vi.mock("../../components/ContextMenu/ContextMenu", () => ({
  showContextMenu: vi.fn(),
}));

import TabBar from "../../components/Editor/TabBar";

const NOTE = {
  path: "/home/user/Writ/away.md",
  title: "away.md",
  provider: "iCloud Drive",
};

describe("TabBar — a note waiting on its bytes", () => {
  afterEach(() => {
    mocks.cancelMaterialiseNote.mockClear();
    downloads.close(NOTE.path);
    cleanup();
  });

  it("shows the note with a marker and stops the wait when its tab is closed", async () => {
    await downloads.start(NOTE);
    const { container } = render(() => <TabBar />);

    const tab = container.querySelector(".tab-download");
    expect(tab).not.toBeNull();
    expect(tab!.querySelector(".tab-download-marker")!.textContent).toBe("downloading");

    const dismiss = tab!.querySelector<HTMLElement>(".tab-close")!;
    expect(dismiss.getAttribute("aria-label")).toBe("Cancel away.md");
    fireEvent.click(dismiss);

    await waitFor(() => expect(mocks.cancelMaterialiseNote).toHaveBeenCalledWith(NOTE.path));
    expect(container.querySelector(".tab-download")).toBeNull();
  });

  it("closes a note that stopped without asking to cancel a wait that ended", async () => {
    await downloads.start(NOTE);
    await downloads.handle({ path: NOTE.path, state: "timed_out" });
    const { container } = render(() => <TabBar />);

    const dismiss = container.querySelector<HTMLElement>(".tab-download .tab-close")!;
    expect(dismiss.getAttribute("aria-label")).toBe("Close away.md");
    fireEvent.click(dismiss);

    await waitFor(() => expect(container.querySelector(".tab-download")).toBeNull());
    expect(mocks.cancelMaterialiseNote).not.toHaveBeenCalled();
  });
});
