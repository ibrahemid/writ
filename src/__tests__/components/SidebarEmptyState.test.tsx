import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
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
let historyRows: BufferDocument[] = [];

vi.mock("../../services/autosave", () => ({ flushAutosave: vi.fn() }));
vi.mock("../../components/ContextMenu/ContextMenu", () => ({
  showContextMenu: vi.fn(),
}));
vi.mock("../../services/tauri", () => ({
  listActiveBuffers: vi.fn(async () => activeRows),
  listHistory: vi.fn(async () => historyRows),
  restoreBuffer: vi.fn(async () => {}),
  deleteBuffer: vi.fn(async () => {}),
  closeBuffer: vi.fn(async () => {}),
  clearHistory: vi.fn(async () => {}),
  renameBuffer: vi.fn(async () => {}),
  createBuffer: vi.fn(),
  searchBuffers: vi.fn(async () => [] as string[]),
}));

afterEach(() => {
  cleanup();
  activeRows = [];
  historyRows = [];
});

async function renderSidebar(windowId: number) {
  const { bufferRegistry } = await import("../../stores/global/buffer-registry");
  const WindowProvider = (await import("../../components/WindowProvider/WindowProvider")).default;
  const Sidebar = (await import("../../components/Sidebar/Sidebar")).default;
  await bufferRegistry.load();
  return render(() => (
    <WindowProvider windowId={windowId}>
      <Sidebar />
    </WindowProvider>
  ));
}

describe("sidebar empty state", () => {
  it("shows a designed empty state when there are no open files and no history", async () => {
    activeRows = [];
    historyRows = [];
    const { container } = await renderSidebar(9501);

    const empty = container.querySelector(".sidebar-empty");
    expect(empty).toBeTruthy();
    expect(container.textContent).toContain("No open files");
    // The cold front door points at the two ways to get a buffer.
    expect(container.querySelector(".sidebar-empty .kbd-chord")).toBeTruthy();
  });

  it("does not show the empty state when history exists", async () => {
    activeRows = [];
    historyRows = [histDoc];
    const { container } = await renderSidebar(9502);

    expect(container.querySelector(".sidebar-empty")).toBeNull();
    expect(container.querySelector(".history-section")).toBeTruthy();
  });

  it("does not show the empty state when active tabs exist", async () => {
    activeRows = [{ ...histDoc, id: "a1", status: "active", closed_at: null }];
    historyRows = [];
    const { container } = await renderSidebar(9503);

    expect(container.querySelector(".sidebar-empty")).toBeNull();
    expect(container.querySelector(".active-section")).toBeTruthy();
  });

  it("does not show the empty state while searching, even with no buffers", async () => {
    activeRows = [];
    historyRows = [];
    const { windowRegistry } = await import("../../stores/global/window-registry");
    const { container } = await renderSidebar(9504);
    expect(container.querySelector(".sidebar-empty")).toBeTruthy();

    windowRegistry.getActive()!.sidebar.setSearchQuery("anything");
    // Searching swaps the non-search fallback (which owns the empty state) for
    // the results branch, so the empty void can never co-exist with a query.
    expect(container.querySelector(".sidebar-empty")).toBeNull();
  });
});
