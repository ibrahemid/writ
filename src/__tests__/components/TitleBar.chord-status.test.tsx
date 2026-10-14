import { describe, it, expect, vi, beforeAll, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import { createSignal } from "solid-js";

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
});

const h = vi.hoisted(() => ({
  openShortcutEditor: vi.fn(),
  hotkey: null as null | {
    setTaken: (taken: boolean) => void;
    setUnsupported: (unsupported: boolean) => void;
  },
}));

vi.mock("../../lib/platform", () => ({
  detectPlatform: () => "win",
  resolvePlatform: () => "win",
  IS_MAC: false,
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: { isRemovedOnDisk: () => false },
    tabs: { activeTabId: () => null },
    downloads: { pending: () => [], selectedPath: () => null },
  }),
}));

vi.mock("../../stores/global/os-window", () => ({
  osWindowStore: {
    focused: () => true,
    maximized: () => false,
    snapHovered: () => false,
    snapPressed: () => false,
    toggleMaximize: vi.fn(),
    startDragging: vi.fn(),
    minimize: vi.fn(),
    hide: vi.fn(),
    toggleFullscreen: vi.fn(),
    installSnapOverlay: vi.fn(),
  },
}));

vi.mock("../../stores/global/config", () => ({
  configStore: { config: () => ({ hotkey: { toggle: "CmdOrCtrl+Shift+Space" } }) },
}));

vi.mock("../../stores/global/hotkey", async () => {
  const { createRoot } = await import("solid-js");
  return createRoot(() => {
    const [taken, setTaken] = createSignal(false);
    const [unsupported, setUnsupported] = createSignal(false);
    h.hotkey = { setTaken, setUnsupported };
    return {
      SHIPPED_WINDOW_CHORD: "CmdOrCtrl+Shift+Space",
      hotkeyStore: {
        chord: () => "CmdOrCtrl+Shift+Space",
        isTaken: taken,
        isUnsupported: unsupported,
      },
    };
  });
});

vi.mock("../../components/ShortcutEditor/ShortcutEditor", () => ({
  openShortcutEditor: h.openShortcutEditor,
  WAYLAND_LINE: "Wayland doesn't pass this shortcut to Writ from other apps.",
}));

import TitleBar from "../../components/TitleBar/TitleBar";

function chip(container: HTMLElement): HTMLElement {
  const found = container.querySelector<HTMLElement>(".titlebar-right");
  expect(found).not.toBeNull();
  return found!;
}

describe("the titlebar keycap for the window's chord", () => {
  afterEach(() => {
    h.hotkey?.setTaken(false);
    h.hotkey?.setUnsupported(false);
    h.openShortcutEditor.mockReset();
    cleanup();
  });

  it("shows the chord plainly while Writ holds it", () => {
    const { container } = render(() => <TitleBar />);
    expect(chip(container).tagName).toBe("DIV");
    expect(chip(container).querySelector(".kbd-chord-muted")).toBeNull();
  });

  it("goes muted, says another app holds it and opens the recorder, when it is taken", () => {
    h.hotkey!.setTaken(true);
    const { container } = render(() => <TitleBar />);

    const button = chip(container);
    expect(button.tagName).toBe("BUTTON");
    expect(button.dataset.state).toBe("taken");
    expect(button.querySelector(".kbd-chord-muted")).not.toBeNull();
    expect(button.getAttribute("aria-label")).toBe("Another app is using Ctrl+Shift+Space");

    fireEvent.click(button);
    expect(h.openShortcutEditor).toHaveBeenCalledWith({ recordWindowChord: true });
  });

  it("follows the chord coming free again", () => {
    h.hotkey!.setTaken(true);
    const { container } = render(() => <TitleBar />);
    expect(chip(container).tagName).toBe("BUTTON");

    h.hotkey!.setTaken(false);
    expect(chip(container).tagName).toBe("DIV");
  });

  it("says a Wayland session cannot pass the chord on", () => {
    h.hotkey!.setUnsupported(true);
    const { container } = render(() => <TitleBar />);

    const button = chip(container);
    expect(button.dataset.state).toBe("unsupported");
    expect(button.getAttribute("aria-label")).toBe(
      "Wayland doesn't pass this shortcut to Writ from other apps.",
    );
    fireEvent.click(button);
    expect(h.openShortcutEditor).toHaveBeenCalledWith({ recordWindowChord: false });
  });
});
