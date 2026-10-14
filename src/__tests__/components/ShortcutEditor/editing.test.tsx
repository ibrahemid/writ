import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

// What the editor tells the user about a chord more than one command wants, and what it
// does with recorded work on the way out.

const h = vi.hoisted(() => ({
  focusEditor: vi.fn(),
  saveConfig: vi.fn().mockResolvedValue(undefined),
  keybindings: {} as Record<string, string>,
  showToast: vi.fn(),
}));

vi.mock("../../../stores/global/config", () => ({
  configStore: {
    config: () => ({
      keybindings: h.keybindings,
      hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
    }),
    save: h.saveConfig,
    isAppOn: () => false,
  },
}));

vi.mock("../../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: h.focusEditor } }),
}));

vi.mock("../../../stores/global/notifications", () => ({
  showToast: h.showToast,
  default: () => null,
}));

import ShortcutEditor, {
  openShortcutEditor,
  closeShortcutEditor,
} from "../../../components/ShortcutEditor/ShortcutEditor";
import ConfirmDialog from "../../../components/ConfirmDialog/ConfirmDialog";
import { registerCommand, getAllCommands, unregisterCommand } from "../../../commands/registry";

function both() {
  return (
    <>
      <ShortcutEditor />
      <ConfirmDialog />
    </>
  );
}

beforeEach(() => {
  for (const cmd of [...getAllCommands()]) unregisterCommand(cmd.id);
  h.keybindings = {};
  h.showToast.mockReset();
  h.saveConfig.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  closeShortcutEditor();
  cleanup();
});

function manyOnOneChord() {
  registerCommand({
    id: "note.new",
    label: "New note",
    keybinding: "CmdOrCtrl+N",
    scope: "app",
    execute: vi.fn(),
  });
  registerCommand({
    id: "settings.open",
    label: "Settings",
    keybinding: "CmdOrCtrl+N",
    scope: "app",
    execute: vi.fn(),
  });
  registerCommand({
    id: "note.today",
    label: "Today's note",
    keybinding: "CmdOrCtrl+N",
    scope: "app",
    execute: vi.fn(),
  });
}

describe("a chord more than one command wants", () => {
  it("names the commands the way the user knows them", () => {
    manyOnOneChord();
    const screen = render(both);
    openShortcutEditor();

    const lines = Array.from(screen.container.querySelectorAll(".shortcut-row-conflict")).map(
      (el) => el.textContent ?? "",
    );
    expect(lines).toHaveLength(3);
    expect(lines[0]).toBe("Conflicts with Settings and Today's note");
    for (const line of lines) {
      for (const id of ["note.new", "settings.open", "note.today"]) {
        expect(line).not.toContain(id);
      }
    }
  });

  it("holds Save back until the chord belongs to one command", () => {
    manyOnOneChord();
    const screen = render(both);
    openShortcutEditor();

    const save = screen.container.querySelector<HTMLButtonElement>("[data-action='save-shortcuts']")!;
    expect(save.disabled).toBe(true);
    expect(screen.container.querySelector(".shortcut-editor-blocked")!.textContent).toContain(
      "Two commands share a shortcut.",
    );
  });
});

describe("leaving the editor with recorded work", () => {
  it("asks before it throws the changes away", async () => {
    registerCommand({
      id: "note.new",
      label: "New note",
      keybinding: "CmdOrCtrl+N",
      scope: "app",
      execute: vi.fn(),
    });
    h.keybindings = { "note.new": "CmdOrCtrl+Shift+N" };

    const screen = render(both);
    openShortcutEditor();
    fireEvent.click(screen.container.querySelector("[data-action='reset-all-shortcuts']")!);

    fireEvent.click(screen.container.querySelector(".shortcut-editor-overlay")!);

    await waitFor(() => expect(screen.container.querySelector(".confirm-dialog")).toBeTruthy());
    expect(screen.container.querySelector(".shortcut-editor")).toBeTruthy();
    expect(screen.getByText("Discard your changes?")).toBeTruthy();

    fireEvent.click(screen.container.querySelector(".confirm-cancel")!);
    await waitFor(() => expect(screen.container.querySelector(".confirm-dialog")).toBeNull());
    expect(screen.container.querySelector(".shortcut-editor")).toBeTruthy();
  });

  it("closes without asking when nothing was recorded", async () => {
    registerCommand({
      id: "note.new",
      label: "New note",
      keybinding: "CmdOrCtrl+N",
      scope: "app",
      execute: vi.fn(),
    });

    const screen = render(both);
    openShortcutEditor();
    fireEvent.click(screen.container.querySelector(".shortcut-editor-overlay")!);

    await waitFor(() => expect(screen.container.querySelector(".shortcut-editor")).toBeNull());
    expect(screen.container.querySelector(".confirm-dialog")).toBeNull();
  });
});

describe("the editor's own words", () => {
  it("writes an empty binding in sentence case", () => {
    registerCommand({
      id: "note.today",
      label: "Today's note",
      scope: "app",
      execute: vi.fn(),
    });

    const screen = render(both);
    openShortcutEditor();

    expect(screen.container.querySelector(".shortcut-row-empty")!.textContent).toBe("Not set");
  });

  it("takes its name from the title on screen", () => {
    const screen = render(both);
    openShortcutEditor();

    const dialog = screen.container.querySelector<HTMLElement>(".shortcut-editor")!;
    expect(dialog.getAttribute("aria-label")).toBeNull();
    const titleId = dialog.getAttribute("aria-labelledby")!;
    expect(screen.container.querySelector(`#${titleId}`)!.textContent).toBe("Customize shortcuts");
  });

  it("says it could not save the shortcuts", async () => {
    registerCommand({
      id: "note.new",
      label: "New note",
      keybinding: "CmdOrCtrl+N",
      scope: "app",
      execute: vi.fn(),
    });
    h.saveConfig.mockRejectedValueOnce(new Error("disk"));

    const screen = render(both);
    openShortcutEditor();
    fireEvent.click(screen.container.querySelector("[data-action='save-shortcuts']")!);

    await waitFor(() => expect(h.showToast).toHaveBeenCalled());
    expect(h.showToast).toHaveBeenCalledWith("Could not save the shortcuts", "error");
  });
});

describe("a command whose app is off", () => {
  function chatAndNew() {
    registerCommand({
      id: "chat.toggle",
      label: "Toggle chat",
      keybinding: "CmdOrCtrl+Shift+A",
      scope: "app",
      app: "chat",
      execute: vi.fn(),
    });
    registerCommand({
      id: "note.new",
      label: "New note",
      keybinding: "CmdOrCtrl+N",
      scope: "app",
      execute: vi.fn(),
    });
    h.keybindings = { "chat.toggle": "CmdOrCtrl+Alt+C" };
  }

  it("is not listed, and leaving with nothing edited asks nothing", async () => {
    chatAndNew();
    const screen = render(both);
    openShortcutEditor();
    expect(screen.queryByText("Toggle chat")).toBeNull();
    fireEvent.click(screen.container.querySelector(".shortcut-editor-overlay")!);
    await waitFor(() => expect(screen.container.querySelector(".shortcut-editor")).toBeNull());
    expect(screen.container.querySelector(".confirm-dialog")).toBeNull();
  });

  it("keeps its saved chord when the editor saves", async () => {
    chatAndNew();
    const screen = render(both);
    openShortcutEditor();
    fireEvent.click(screen.container.querySelector("[data-action='save-shortcuts']")!);
    await waitFor(() => expect(h.saveConfig).toHaveBeenCalled());
    expect(h.saveConfig.mock.calls[0][0].keybindings).toEqual({ "chat.toggle": "CmdOrCtrl+Alt+C" });
  });
});

describe("chords the macOS menu bar holds", () => {
  afterEach(() => {
    delete (window.navigator as unknown as Record<string, unknown>).platform;
  });

  it("counts Quit's chord as taken and holds Save back", () => {
    Object.defineProperty(window.navigator, "platform", { value: "MacIntel", configurable: true });
    registerCommand({
      id: "note.new",
      label: "New note",
      keybinding: "CmdOrCtrl+N",
      scope: "app",
      execute: vi.fn(),
    });
    h.keybindings = { "note.new": "CmdOrCtrl+Q" };

    const screen = render(both);
    openShortcutEditor();

    expect(screen.container.querySelector(".shortcut-row-conflict")!.textContent).toBe(
      "Conflicts with Quit Writ",
    );
    const save = screen.container.querySelector<HTMLButtonElement>("[data-action='save-shortcuts']")!;
    expect(save.disabled).toBe(true);
    expect(screen.container.querySelector(".shortcut-editor-blocked")!.textContent).toBe(
      "The menu bar already uses a shortcut.",
    );
  });

  it("leaves the chord free where there is no menu bar", () => {
    Object.defineProperty(window.navigator, "platform", { value: "Win32", configurable: true });
    registerCommand({
      id: "note.new",
      label: "New note",
      keybinding: "CmdOrCtrl+N",
      scope: "app",
      execute: vi.fn(),
    });
    h.keybindings = { "note.new": "CmdOrCtrl+Q" };

    const screen = render(both);
    openShortcutEditor();

    expect(screen.container.querySelector(".shortcut-row-conflict")).toBeNull();
  });
});

describe("finding and reading rows", () => {
  function twoCommands() {
    registerCommand({
      id: "note.new",
      label: "New note",
      keybinding: "CmdOrCtrl+N",
      scope: "app",
      execute: vi.fn(),
    });
    registerCommand({
      id: "sidebar.toggle",
      label: "Toggle sidebar",
      keybinding: "CmdOrCtrl+\\",
      scope: "app",
      execute: vi.fn(),
    });
  }

  const labels = (container: HTMLElement) =>
    Array.from(container.querySelectorAll(".shortcut-row-label")).map((el) => el.textContent);

  it("narrows the rows to the ones whose name matches the filter", () => {
    twoCommands();
    const screen = render(both);
    openShortcutEditor();

    const filter = screen.getByLabelText("Filter shortcuts") as HTMLInputElement;
    fireEvent.input(filter, { target: { value: "sidebar" } });
    expect(labels(screen.container)).toEqual(["Toggle sidebar"]);

    fireEvent.input(filter, { target: { value: "nothing like this" } });
    expect(labels(screen.container)).toEqual([]);
    expect(screen.getByText("No shortcuts match.")).toBeTruthy();
  });

  it("marks a row whose chord is not the shipped one", () => {
    twoCommands();
    h.keybindings = { "note.new": "CmdOrCtrl+Shift+N" };
    const screen = render(both);
    openShortcutEditor();

    const marked = Array.from(screen.container.querySelectorAll(".shortcut-row"))
      .filter((row) => row.querySelector(".shortcut-row-changed"))
      .map((row) => row.querySelector(".shortcut-row-label")!.textContent);
    expect(marked).toEqual(["New note"]);
  });

  it("shows each chord as keycaps with a spoken name", () => {
    twoCommands();
    const screen = render(both);
    openShortcutEditor();

    const names = Array.from(
      screen.container.querySelectorAll(".shortcut-row-chip .kbd-chord"),
    ).map((el) => el.getAttribute("aria-label"));
    expect(names).toContain("Control N");
  });
});
