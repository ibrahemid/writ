import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

const h = vi.hoisted(() => ({ executeCommand: vi.fn(() => true) }));

vi.mock("../../commands/registry", () => ({
  executeCommand: h.executeCommand,
  getCommand: (id: string) =>
    id === "note.new" ? { id, label: "New file", keybinding: "CmdOrCtrl+N" } : undefined,
  registryVersion: () => 0,
}));

import SidebarEmpty from "../../components/Sidebar/SidebarEmpty";
import { setKeybindingOverrides } from "../../commands/effective-bindings";

function buttonNamed(container: HTMLElement, text: string): HTMLButtonElement {
  const found = Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find(
    (b) => b.textContent?.trim() === text,
  );
  if (!found) throw new Error(`no button named ${text}`);
  return found;
}

afterEach(() => {
  h.executeCommand.mockClear();
  setKeybindingOverrides({});
  cleanup();
});

describe("SidebarEmpty", () => {
  it("says the folder is empty and offers New file and Show folder as buttons", () => {
    const { container } = render(() => <SidebarEmpty />);
    expect(container.querySelector(".sidebar-empty-title")!.textContent).toBe(
      "This folder is empty.",
    );
    expect(buttonNamed(container, "New file").classList).toContain("writ-btn-primary");
    expect(buttonNamed(container, "Show folder in Finder").classList).toContain(
      "writ-btn-secondary",
    );
    // Only New file has a binding.
    const caps = Array.from(container.querySelectorAll(".kbd-chord")).map((el) =>
      el.getAttribute("aria-label"),
    );
    expect(caps).toEqual(["Control N"]);
  });

  it("shows the chord New file answers to now, not the shipped one", () => {
    setKeybindingOverrides({ "note.new": "CmdOrCtrl+Shift+N" });
    const { container } = render(() => <SidebarEmpty />);
    expect(container.querySelector(".kbd-chord")!.getAttribute("aria-label")).toBe(
      "Control Shift N",
    );
  });

  it("New file runs the note.new command", () => {
    const { container } = render(() => <SidebarEmpty />);
    fireEvent.click(buttonNamed(container, "New file"));
    expect(h.executeCommand).toHaveBeenCalledWith("note.new");
  });

  it("Show folder runs the command that shows the notes folder", () => {
    const { container } = render(() => <SidebarEmpty />);
    fireEvent.click(buttonNamed(container, "Show folder in Finder"));
    expect(h.executeCommand).toHaveBeenCalledWith("notes.showFolder");
  });

  it("never says buffer or scratch", () => {
    const { container } = render(() => <SidebarEmpty />);
    expect(container.textContent!.toLowerCase()).not.toMatch(/buffer|scratch/);
  });
});
