import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = function scrollIntoView() {};
  }
});

const h = vi.hoisted(() => ({
  usage: {} as Record<string, unknown>,
  recordCommandUse: vi.fn(),
  openSettings: vi.fn(),
  execute: vi.fn(),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({ commands: { usage: h.usage } }),
    recordCommandUse: h.recordCommandUse,
  },
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: vi.fn() } }),
}));

vi.mock("../../components/SettingsModal/SettingsModal", () => ({
  openSettings: h.openSettings,
  default: () => null,
}));

vi.mock("../../settings/availability", () => ({
  isSettingAvailable: () => true,
}));

import CommandPalette, {
  openCommandPalette,
  closeCommandPalette,
} from "../../components/CommandPalette/CommandPalette";
import { registerCommand, getAllCommands, unregisterCommand } from "../../commands/registry";

function input(): HTMLInputElement {
  return document.querySelector<HTMLInputElement>(".palette-input")!;
}

function items(): HTMLElement[] {
  return Array.from(document.querySelectorAll<HTMLElement>(".palette-item"));
}

async function open() {
  render(() => <CommandPalette />);
  openCommandPalette();
  await waitFor(() => expect(document.querySelector(".palette-input")).not.toBeNull());
}

// The command palette after the shell extraction: it is a composition over the
// shared Palette, so the behavior Track A left it with has to survive intact.
describe("CommandPalette over the shared shell", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    for (const cmd of [...getAllCommands()]) unregisterCommand(cmd.id);
    registerCommand({
      id: "cmd.alpha",
      label: "Alpha command",
      scope: "app",
      keybinding: "CmdOrCtrl+1",
      execute: h.execute,
    });
    registerCommand({
      id: "cmd.editor",
      label: "Editor thing",
      scope: "editor",
      execute: vi.fn(),
    });
    registerCommand({
      id: "palette.open",
      label: "Command Palette",
      scope: "app",
      execute: vi.fn(),
    });
  });

  afterEach(() => {
    closeCommandPalette();
    cleanup();
    while (document.body.firstChild) document.body.removeChild(document.body.firstChild);
  });

  it("subdivides commands and editor commands on an empty query", async () => {
    await open();
    const labels = Array.from(document.querySelectorAll(".palette-section-label")).map(
      (el) => el.textContent,
    );
    expect(labels).toEqual(["Commands", "Editor"]);
  });

  it("never lists its own opener", async () => {
    await open();
    const rows = items().map((el) => el.textContent ?? "");
    expect(rows.some((t) => t.includes("Command Palette"))).toBe(false);
  });

  it("records usage exactly once for a command run from the palette", async () => {
    await open();
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(h.execute).toHaveBeenCalledTimes(1);
    expect(h.recordCommandUse).toHaveBeenCalledTimes(1);
    expect(h.recordCommandUse).toHaveBeenCalledWith("cmd.alpha");
  });

  it("shows the shortcut column for command rows", async () => {
    await open();
    expect(items()[0].querySelector(".kbd-chord")).not.toBeNull();
  });

  it("exposes listbox semantics and tracks the active option", async () => {
    await open();
    expect(document.querySelector(".palette-results")?.getAttribute("role")).toBe("listbox");
    const rows = items();
    expect(input().getAttribute("aria-activedescendant")).toBe(rows[0].id);
    fireEvent.keyDown(input(), { key: "ArrowDown" });
    expect(input().getAttribute("aria-activedescendant")).toBe(rows[1].id);
  });

  it("clears the query when it closes", async () => {
    await open();
    fireEvent.input(input(), { target: { value: "alpha" } });
    expect(input().value).toBe("alpha");
    closeCommandPalette();
    await waitFor(() => expect(document.querySelector(".palette")).toBeNull());
    openCommandPalette();
    await waitFor(() => expect(document.querySelector(".palette-input")).not.toBeNull());
    expect(input().value).toBe("");
  });
});
