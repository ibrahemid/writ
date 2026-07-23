import { describe, it, expect, vi, afterEach, beforeEach, beforeAll } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = function scrollIntoView() {};
  }
});

const h = vi.hoisted(() => ({
  recordCommandUse: vi.fn(),
  save: vi.fn().mockResolvedValue(undefined),
  focusEditor: vi.fn(),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({ commands: { usage: {} }, keybindings: {} }),
    recordCommandUse: h.recordCommandUse,
    save: h.save,
  },
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: h.focusEditor } }),
}));

vi.mock("../../components/SettingsModal/SettingsModal", () => ({
  openSettings: vi.fn(),
  default: () => null,
}));

vi.mock("../../settings/availability", () => ({
  isSettingAvailable: () => true,
}));

vi.mock("../Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

import CommandPalette, {
  openCommandPalette,
  closeCommandPalette,
} from "../../components/CommandPalette/CommandPalette";
import ShortcutEditor, {
  openShortcutEditor,
  closeShortcutEditor,
} from "../../components/ShortcutEditor/ShortcutEditor";
import {
  registerCommand,
  getAllCommands,
  unregisterCommand,
} from "../../commands/registry";
import {
  rebuildKeyMap,
  handleKeyDown,
  setKeybindingOverrides,
} from "../../commands/keybindings";

function clearRegistry() {
  for (const cmd of [...getAllCommands()]) unregisterCommand(cmd.id);
}

function resetDom() {
  while (document.body.firstChild) document.body.removeChild(document.body.firstChild);
}

describe("editor commands in the command palette", () => {
  let dupExecuted = 0;

  beforeEach(() => {
    clearRegistry();
    h.recordCommandUse.mockClear();
    dupExecuted = 0;
    registerCommand({
      id: "cmd.alpha",
      label: "Alpha command",
      scope: "app",
      execute: vi.fn(),
    });
    registerCommand({
      id: "editor.duplicateLine",
      label: "Duplicate Line",
      keybinding: "CmdOrCtrl+D",
      scope: "editor",
      execute: () => {
        dupExecuted += 1;
        return true;
      },
    });
    rebuildKeyMap();
  });

  afterEach(() => {
    closeCommandPalette();
    cleanup();
    resetDom();
  });

  async function open() {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const user = userEvent.setup({ document });
    render(() => <CommandPalette />, { container });
    openCommandPalette();
    await new Promise<void>((r) => setTimeout(r, 0));
    return user;
  }

  it("lists an editor command under the Editor section with its chord", async () => {
    await open();
    await waitFor(() =>
      expect(document.querySelector(".palette-section-all")).not.toBeNull(),
    );
    const labels = Array.from(
      document.querySelectorAll(".palette-section-label"),
    ).map((el) => el.textContent);
    expect(labels).toContain("Editor");

    const item = Array.from(document.querySelectorAll(".palette-item")).find((el) =>
      el.textContent?.includes("Duplicate Line"),
    );
    expect(item).toBeDefined();
    expect(item!.querySelector('[aria-label="CmdOrCtrl+D"]')).not.toBeNull();
  });

  it("executes an editor command on Enter and records its use", async () => {
    const user = await open();
    await user.keyboard("Duplicate");
    await waitFor(() =>
      expect(
        document.querySelector(".palette-item.is-selected")?.textContent,
      ).toContain("Duplicate Line"),
    );
    await user.keyboard("{Enter}");
    expect(dupExecuted).toBe(1);
    expect(h.recordCommandUse).toHaveBeenCalledWith("editor.duplicateLine");
  });
});

describe("editor commands in the shortcut editor", () => {
  let dupExecuted = 0;
  let editor: HTMLDivElement;

  beforeEach(() => {
    clearRegistry();
    h.save.mockClear();
    dupExecuted = 0;
    registerCommand({
      id: "editor.duplicateLine",
      label: "Duplicate Line",
      keybinding: "CmdOrCtrl+D",
      scope: "editor",
      execute: () => {
        dupExecuted += 1;
        return true;
      },
    });
    rebuildKeyMap();
    editor = document.createElement("div");
    editor.className = "cm-editor";
    editor.tabIndex = -1;
    document.body.appendChild(editor);
  });

  afterEach(() => {
    closeShortcutEditor();
    cleanup();
    resetDom();
    document.body.focus();
    setKeybindingOverrides({});
    rebuildKeyMap();
  });

  async function open() {
    const container = document.createElement("div");
    container.id = "app";
    document.body.appendChild(container);
    const user = userEvent.setup({ document });
    render(() => <ShortcutEditor />, { container });
    openShortcutEditor();
    await new Promise<void>((r) => setTimeout(r, 0));
    return user;
  }

  it("lists the editor command under an Editor group", async () => {
    await open();
    const groups = Array.from(
      document.querySelectorAll(".shortcut-group-label"),
    ).map((el) => el.textContent);
    expect(groups).toContain("Editor");
    const rows = Array.from(document.querySelectorAll(".shortcut-row-label")).map(
      (el) => el.textContent,
    );
    expect(rows).toContain("Duplicate Line");
  });

  it("rebinds the editor command and the new chord takes effect", async () => {
    const user = await open();

    const row = Array.from(document.querySelectorAll(".shortcut-row")).find((el) =>
      el.textContent?.includes("Duplicate Line"),
    )!;
    const recordBtn = Array.from(
      row.querySelectorAll<HTMLButtonElement>(".shortcut-row-btn"),
    ).find((b) => b.textContent?.trim() === "Record")!;
    await user.click(recordBtn);
    await new Promise<void>((r) => setTimeout(r, 0));

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "y", metaKey: true, bubbles: true }),
    );
    await waitFor(() =>
      expect(row.querySelector(".shortcut-row-listening")).toBeNull(),
    );

    const saveBtn = Array.from(
      document.querySelectorAll<HTMLButtonElement>(".shortcut-editor-btn"),
    ).find((b) => b.textContent?.trim() === "Save")!;
    await user.click(saveBtn);
    await waitFor(() => expect(h.save).toHaveBeenCalled());

    // Close the editor (the save is retained via its snapshot) so the modal
    // guard no longer swallows the keystroke, as it would in the real app.
    closeShortcutEditor();
    await new Promise<void>((r) => setTimeout(r, 0));

    editor.focus();
    expect(editor.contains(document.activeElement)).toBe(true);

    const rebound = {
      key: "y",
      metaKey: true,
      ctrlKey: false,
      shiftKey: false,
      altKey: false,
      isComposing: false,
      keyCode: 0,
      preventDefault: vi.fn(),
      stopPropagation: vi.fn(),
    } as unknown as KeyboardEvent;
    expect(handleKeyDown(rebound)).toBe(true);
    expect(dupExecuted).toBe(1);

    const old = {
      key: "d",
      metaKey: true,
      ctrlKey: false,
      shiftKey: false,
      altKey: false,
      isComposing: false,
      keyCode: 0,
      preventDefault: vi.fn(),
      stopPropagation: vi.fn(),
    } as unknown as KeyboardEvent;
    expect(handleKeyDown(old)).toBe(false);
    expect(dupExecuted).toBe(1);
  });
});
