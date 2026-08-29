import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

const h = vi.hoisted(() => ({ executeCommand: vi.fn(() => true) }));

vi.mock("../../commands/registry", () => ({ executeCommand: h.executeCommand }));

import SidebarEmpty from "../../components/Sidebar/SidebarEmpty";

function buttonNamed(container: HTMLElement, text: string): HTMLButtonElement {
  const found = Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find(
    (b) => b.textContent?.trim() === text,
  );
  if (!found) throw new Error(`no button named ${text}`);
  return found;
}

afterEach(() => {
  h.executeCommand.mockClear();
  cleanup();
});

describe("SidebarEmpty", () => {
  it("offers New note and Open a folder as buttons with their keycaps", () => {
    const { container } = render(() => <SidebarEmpty />);
    expect(container.querySelector(".sidebar-empty-title")!.textContent).toBe("No notes yet.");
    expect(buttonNamed(container, "New note").classList).toContain("writ-btn-primary");
    expect(buttonNamed(container, "Open a folder").classList).toContain("writ-btn-secondary");
    const caps = Array.from(container.querySelectorAll(".kbd-chord")).map((el) =>
      el.getAttribute("aria-label"),
    );
    expect(caps).toEqual(["CmdOrCtrl+N", "CmdOrCtrl+O"]);
  });

  it("New note runs the note.new command", () => {
    const { container } = render(() => <SidebarEmpty />);
    fireEvent.click(buttonNamed(container, "New note"));
    expect(h.executeCommand).toHaveBeenCalledWith("note.new");
  });

  it("Open a folder runs the open-folder command", () => {
    const { container } = render(() => <SidebarEmpty />);
    fireEvent.click(buttonNamed(container, "Open a folder"));
    expect(h.executeCommand).toHaveBeenCalledWith("workspace.openFolder");
  });

  it("never says buffer or scratch", () => {
    const { container } = render(() => <SidebarEmpty />);
    expect(container.textContent!.toLowerCase()).not.toMatch(/buffer|scratch/);
  });
});
