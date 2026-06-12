import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, within } from "@solidjs/testing-library";
import type { BufferDocument } from "../../types/buffer";

const histDoc: BufferDocument = {
  id: "h1",
  title: "notes.rs",
  filename: "notes.rs",
  status: "history",
  language: null,
  source_path: "/proj/src/notes.rs",
  cursor_pos: 0,
  scroll_pos: 0,
  tab_order: 0,
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  closed_at: new Date().toISOString(),
  read_only: false,
  size_bytes: 0,
};

let activeRows: BufferDocument[] = [];
let historyRows: BufferDocument[] = [histDoc];

vi.mock("../../services/autosave", () => ({ flushAutosave: vi.fn() }));
vi.mock("../../components/ContextMenu/ContextMenu", () => ({
  showContextMenu: vi.fn(),
}));
vi.mock("../../services/tauri", () => ({
  listActiveBuffers: vi.fn(async () => activeRows),
  listHistory: vi.fn(async () => historyRows),
  restoreBuffer: vi.fn(async () => {
    historyRows = [];
    activeRows = [{ ...histDoc, status: "active", closed_at: null }];
  }),
  deleteBuffer: vi.fn(async () => {}),
  closeBuffer: vi.fn(async () => {}),
  clearHistory: vi.fn(async () => {}),
  renameBuffer: vi.fn(async () => {}),
  createBuffer: vi.fn(),
}));

afterEach(() => {
  cleanup();
  activeRows = [];
  historyRows = [histDoc];
});

describe("sidebar: opening a history buffer keeps it visible", () => {
  it("moves the buffer into the Active section instead of dropping it", async () => {
    const { bufferRegistry } = await import("../../stores/global/buffer-registry");
    const { windowRegistry } = await import("../../stores/global/window-registry");
    const WindowProvider = (await import("../../components/WindowProvider/WindowProvider")).default;
    const ActiveSection = (await import("../../components/Sidebar/ActiveSection")).default;
    const HistorySection = (await import("../../components/Sidebar/HistorySection")).default;

    await bufferRegistry.load();

    const { container } = render(() => (
      <WindowProvider windowId={9001}>
        <ActiveSection />
        <HistorySection />
      </WindowProvider>
    ));

    const historyBefore = container.querySelector<HTMLElement>(".history-section")!;
    expect(within(historyBefore).queryByText("notes.rs")).toBeTruthy();
    expect(container.querySelector(".active-section")).toBeNull();

    await windowRegistry.getActive()!.tabs.restoreFromHistory("h1");

    const activeAfter = container.querySelector<HTMLElement>(".active-section")!;
    expect(activeAfter).toBeTruthy();
    expect(within(activeAfter).queryByText("notes.rs")).toBeTruthy();
    expect(container.querySelector(".history-section")).toBeNull();
  });
});
