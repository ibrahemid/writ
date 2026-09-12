import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import type { BufferDocument } from "../../types/buffer";

const h = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [collapsed, setCollapsed] = createSignal<string[]>([]);
  return {
    collapsed,
    setCollapsed,
    history: [] as BufferDocument[],
    recentRequest: createSignal(0),
  };
});

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({ sidebar: { collapsed: h.collapsed(), hidden: [] } }),
    isSidebarSectionCollapsed: (id: string) => h.collapsed().includes(id),
    isSidebarSectionHidden: () => false,
    setSidebarSectionCollapsed: (id: string, on: boolean) =>
      h.setCollapsed((list) => (on ? [...list, id] : list.filter((held) => held !== id))),
    setSidebarSectionHidden: vi.fn(),
  },
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    historyList: () => h.history,
    historyTotal: () => h.history.length,
    deleteFromHistory: vi.fn(),
    clearAllHistory: vi.fn(),
  },
}));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    sidebar: { recentRequest: h.recentRequest[0] },
    tabs: { restoreFromHistory: vi.fn() },
  }),
}));
vi.mock("../../components/ContextMenu/ContextMenu", () => ({ showContextMenu: vi.fn() }));

import HistorySection from "../../components/Sidebar/HistorySection";

function doc(id: string, title: string): BufferDocument {
  return {
    id,
    title,
    filename: title,
    status: "history",
    language: null,
    source_path: null,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "2026-08-25T10:00:00.000Z",
    updated_at: "2026-08-25T10:00:00.000Z",
    closed_at: "2026-08-25T09:00:00.000Z",
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
  };
}

/** The sidebar's own scroller, which the list finds by class. */
function mountInScroller() {
  const scroller = document.createElement("div");
  scroller.className = "sidebar-scroll";
  document.body.appendChild(scroller);
  const listen = vi.spyOn(scroller, "addEventListener");
  const unlisten = vi.spyOn(scroller, "removeEventListener");
  render(() => <HistorySection />, { container: scroller });
  return { scroller, listen, unlisten };
}

const scrollCalls = (spy: { mock: { calls: unknown[][] } }) =>
  spy.mock.calls.filter(([type]) => type === "scroll").length;

afterEach(() => {
  h.setCollapsed([]);
  h.history = [];
  cleanup();
});

describe("HistorySection folded", () => {
  it("renders its rows and listens to the scroller once unfolded", () => {
    h.history = [doc("h1", "Kitchen rebuild"), doc("h2", "Pricing draft"), doc("h3", "Reading")];
    h.setCollapsed(["recent"]);
    const { scroller, listen, unlisten } = mountInScroller();

    expect(scroller.querySelector(".sidebar-section-count")!.textContent).toBe("3");
    expect(scroller.querySelectorAll(".tab-item")).toHaveLength(0);
    expect(scrollCalls(listen)).toBe(0);

    fireEvent.click(scroller.querySelector(".sidebar-section-toggle")!);
    expect(scroller.querySelectorAll(".tab-item")).toHaveLength(3);
    expect(scrollCalls(listen)).toBe(1);

    fireEvent.click(scroller.querySelector(".sidebar-section-toggle")!);
    expect(scroller.querySelectorAll(".tab-item")).toHaveLength(0);

    fireEvent.click(scroller.querySelector(".sidebar-section-toggle")!);
    expect(scroller.querySelectorAll(".tab-item")).toHaveLength(3);
    expect(scrollCalls(unlisten)).toBeGreaterThanOrEqual(1);
    expect(scrollCalls(listen)).toBe(2);
  });

  it("is a section the recent request can scroll to and focus", () => {
    h.history = [doc("h1", "Kitchen rebuild")];
    const { scroller } = mountInScroller();
    const section = scroller.querySelector<HTMLElement>("section.history-section")!;
    expect(section.getAttribute("tabindex")).toBe("-1");
    section.scrollIntoView = vi.fn();

    h.recentRequest[1]((count) => count + 1);
    expect(section.scrollIntoView).toHaveBeenCalledWith({ block: "nearest" });
    expect(document.activeElement).toBe(section);
  });
});
