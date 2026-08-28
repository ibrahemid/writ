import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

vi.mock("../../stores/global/os-window", () => ({
  osWindowStore: { focused: () => true, maximized: () => false },
}));

import AppMenu from "../../components/TitleBar/AppMenu";
import ContextMenu from "../../components/ContextMenu/ContextMenu";
import { registerCommand, unregisterCommand } from "../../commands/registry";

/**
 * Runs the real ContextMenu, not a mock: the thing under test is what happens
 * after `activate()` calls the action and then closes the menu.
 */
function renderMenu() {
  return render(() => (
    <>
      <AppMenu />
      <ContextMenu />
    </>
  ));
}

afterEach(() => {
  for (const id of ["palette.open", "note.new"]) unregisterCommand(id);
  cleanup();
});

describe("AppMenu focus handoff", () => {
  it("leaves focus with the surface a command opened, not the menu button", async () => {
    // Models the real palette, which focuses its input inside a
    // requestAnimationFrame (Palette.tsx). ContextMenu.close() restores focus to
    // the trigger synchronously, so a deferred focus lands after it and wins. A
    // command that focused synchronously would lose it: see the note in AppMenu.
    const field = document.createElement("input");
    document.body.appendChild(field);
    registerCommand({
      id: "palette.open",
      label: "Command Palette",
      scope: "app",
      execute: () => {
        requestAnimationFrame(() => field.focus());
      },
    });

    try {
      const { container } = renderMenu();
      fireEvent.click(container.querySelector(".titlebar-appmenu")!);

      const entry = Array.from(document.querySelectorAll<HTMLButtonElement>(".context-menu-item"))
        .find((el) => el.textContent?.includes("Command Palette"));
      expect(entry).toBeDefined();
      fireEvent.click(entry!);

      await new Promise((resolve) => requestAnimationFrame(resolve));

      expect(document.activeElement).toBe(field);
      expect(document.activeElement).not.toBe(container.querySelector(".titlebar-appmenu"));
    } finally {
      field.remove();
    }
  });

  it("returns focus to the menu button when the menu is dismissed without acting", async () => {
    registerCommand({
      id: "note.new",
      label: "New Note",
      scope: "app",
      execute: () => {},
    });

    const { container } = renderMenu();
    const button = container.querySelector<HTMLButtonElement>(".titlebar-appmenu")!;
    fireEvent.click(button);

    const menu = document.querySelector(".context-menu")!;
    fireEvent.keyDown(menu, { key: "Escape" });

    await new Promise((resolve) => requestAnimationFrame(resolve));

    expect(document.activeElement).toBe(button);
  });
});
