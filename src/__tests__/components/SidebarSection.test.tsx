import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

const h = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [collapsed, setCollapsed] = createSignal<string[]>([]);
  const [hidden, setHidden] = createSignal<string[]>([]);
  const toggle = (set: typeof setCollapsed) => (id: string, on: boolean) =>
    set((list) => (on ? [...new Set([...list, id])] : list.filter((held) => held !== id)));
  return {
    collapsed,
    setCollapsed,
    hidden,
    setHidden,
    setSidebarSectionCollapsed: vi.fn(toggle(setCollapsed)),
    setSidebarSectionHidden: vi.fn(toggle(setHidden)),
  };
});

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({ sidebar: { collapsed: h.collapsed(), hidden: h.hidden() } }),
    isSidebarSectionCollapsed: (id: string) => h.collapsed().includes(id),
    isSidebarSectionHidden: (id: string) => h.hidden().includes(id),
    setSidebarSectionCollapsed: h.setSidebarSectionCollapsed,
    setSidebarSectionHidden: h.setSidebarSectionHidden,
  },
}));

import SidebarSection from "../../components/Sidebar/SidebarSection";

afterEach(() => {
  h.setCollapsed([]);
  h.setHidden([]);
  h.setSidebarSectionCollapsed.mockClear();
  h.setSidebarSectionHidden.mockClear();
  cleanup();
});

function mount(extra: Partial<Parameters<typeof SidebarSection>[0]> = {}) {
  return render(() => (
    <SidebarSection id="recent" heading="Recently closed" count={3} {...extra}>
      <div class="row">Kitchen rebuild</div>
    </SidebarSection>
  ));
}

describe("SidebarSection", () => {
  it("heads the section with one disclosure button, open by default", () => {
    const { container } = mount();
    const heading = container.querySelector("h2.sidebar-section-heading")!;
    expect(heading).not.toBeNull();
    const toggles = heading.querySelectorAll("button.sidebar-section-toggle");
    expect(toggles).toHaveLength(1);
    expect(toggles[0].getAttribute("type")).toBe("button");
    expect(toggles[0].getAttribute("aria-expanded")).toBe("true");
    expect(toggles[0].querySelector(".sidebar-section-name")!.textContent).toBe("Recently closed");
    expect(toggles[0].querySelector(".sidebar-section-count")!.textContent).toBe("3");
    expect(container.querySelector("section")!.getAttribute("aria-labelledby")).toBe(heading.id);
    expect(container.querySelector(".row")).not.toBeNull();
  });

  it("folds on click through the store, keeping the count in the head", () => {
    const { container } = mount();
    const toggle = container.querySelector<HTMLButtonElement>(".sidebar-section-toggle")!;

    fireEvent.click(toggle);
    expect(h.setSidebarSectionCollapsed).toHaveBeenCalledWith("recent", true);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(container.querySelector(".row")).toBeNull();
    expect(container.querySelector(".sidebar-section-count")!.textContent).toBe("3");

    fireEvent.click(toggle);
    expect(h.setSidebarSectionCollapsed).toHaveBeenLastCalledWith("recent", false);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(container.querySelector(".row")).not.toBeNull();
  });

  it("starts folded when the store says so", () => {
    h.setCollapsed(["recent"]);
    const { container } = mount();
    expect(container.querySelector(".sidebar-section-toggle")!.getAttribute("aria-expanded")).toBe(
      "false",
    );
    expect(container.querySelector(".row")).toBeNull();
  });

  it("renders nothing at all for a hidden section", () => {
    h.setHidden(["recent"]);
    const { container } = mount();
    expect(container.querySelector("section")).toBeNull();
    expect(container.textContent).toBe("");

    h.setHidden([]);
    expect(container.querySelector("section.sidebar-section")).not.toBeNull();
  });

  it("puts the action beside the heading, outside the disclosure", () => {
    const { container } = mount({
      action: <button type="button" class="sidebar-section-action" aria-label="Close folder" />,
    });
    const action = container.querySelector(".sidebar-section-action-slot .sidebar-section-action");
    expect(action).not.toBeNull();
    expect(action!.closest("h2")).toBeNull();
    expect(action!.closest(".sidebar-section-head")).not.toBeNull();
  });

  it("lands the class on the section", () => {
    const { container } = mount({ class: "files-section" });
    const section = container.querySelector("section")!;
    expect(section.classList.contains("sidebar-section")).toBe(true);
    expect(section.classList.contains("files-section")).toBe(true);
  });

  it("is focusable on request and hands the section to the ref", () => {
    let received: HTMLElement | undefined;
    const { container } = mount({ focusable: true, ref: (el) => (received = el) });
    const section = container.querySelector("section")!;
    expect(section.getAttribute("tabindex")).toBe("-1");
    expect(received).toBe(section);
  });

  it("is not in the tab order unless asked", () => {
    const { container } = mount();
    expect(container.querySelector("section")!.hasAttribute("tabindex")).toBe(false);
  });
});
