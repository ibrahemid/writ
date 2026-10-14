import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import type { BufferDocument } from "../../types/buffer";

// A closed file's time reads from the clock, not from the last time the list
// changed: "now" forty minutes later is a false statement (NV-27).

const h = vi.hoisted(() => ({ history: [] as BufferDocument[] }));

vi.mock("../../services/autosave", () => ({
  flushAutosave: vi.fn().mockResolvedValue({ ok: true, failures: [] }),
  onAutosaveStart: vi.fn(() => () => {}),
  onAutosaveSuccess: vi.fn(() => () => {}),
  onAutosaveError: vi.fn(() => () => {}),
}));
vi.mock("../../components/ContextMenu/ContextMenu", () => ({
  showContextMenu: vi.fn(),
}));
vi.mock("../../services/tauri", () => ({
  listActiveBuffers: vi.fn(async () => []),
  listHistory: vi.fn(async () => h.history),
  updateConfig: vi.fn(async () => {}),
  getConfig: vi.fn(async () => ({})),
}));
vi.mock("../../services/events", () => ({
  onEvent: vi.fn().mockResolvedValue(() => {}),
}));

import WindowProvider from "../../components/WindowProvider/WindowProvider";
import HistorySection from "../../components/Sidebar/HistorySection";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { configStore } from "../../stores/global/config";
import { windowRegistry } from "../../stores/global/window-registry";
import { showContextMenu } from "../../components/ContextMenu/ContextMenu";

function closedAt(iso: string): BufferDocument {
  return {
    id: "h1",
    title: "Plan.md",
    filename: "Plan.md",
    status: "history",
    language: null,
    source_path: "/notes/Plan.md",
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: iso,
    updated_at: iso,
    closed_at: iso,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
  };
}

function shownTime(container: HTMLElement): string | null {
  return container.querySelector(".tab-item-trailing")?.textContent ?? null;
}

beforeEach(async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"] });
  vi.setSystemTime(new Date("2026-10-13T09:00:00Z"));
  // The sidebar's open state is the config's, read once it has loaded.
  await configStore.load();
  configStore.setSidebarSectionHidden("recent", false);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("Recent's relative times", () => {
  it("move on with the clock while the sidebar is open", async () => {
    h.history = [closedAt("2026-10-13T09:00:00Z")];
    await bufferRegistry.load();
    const { container } = render(() => (
      <WindowProvider windowId={9801}>
        <HistorySection />
      </WindowProvider>
    ));
    windowRegistry.getActive()!.sidebar.show();
    expect(shownTime(container)).toBe("now");

    vi.advanceTimersByTime(40 * 60_000);

    expect(shownTime(container)).toBe("40m");
  });

  it("read the clock again when the sidebar opens", async () => {
    h.history = [closedAt("2026-10-13T09:00:00Z")];
    await bufferRegistry.load();
    const { container } = render(() => (
      <WindowProvider windowId={9802}>
        <HistorySection />
      </WindowProvider>
    ));
    const sidebar = windowRegistry.getActive()!.sidebar;
    sidebar.hide();
    expect(shownTime(container)).toBe("now");

    vi.setSystemTime(new Date("2026-10-13T11:00:00Z"));
    sidebar.show();

    expect(shownTime(container)).toBe("2h");
  });
});

describe("Recent's menu", () => {
  it("names the list it clears the way its heading does", async () => {
    h.history = [closedAt("2026-10-13T09:00:00Z")];
    await bufferRegistry.load();
    const { container } = render(() => (
      <WindowProvider windowId={9803}>
        <HistorySection />
      </WindowProvider>
    ));

    fireEvent.contextMenu(container.querySelector(".tab-item")!.parentElement!);

    const calls = vi.mocked(showContextMenu).mock.calls;
    const items = calls[calls.length - 1][2] as { label: string }[];
    expect(items.map((item) => item.label)).toContain("Clear Recent");
    expect(items.map((item) => item.label)).not.toContain("Clear all history");
  });
});
