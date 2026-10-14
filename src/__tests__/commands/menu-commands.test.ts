import { describe, it, expect, afterEach } from "vitest";
import { registerCommand, unregisterCommand } from "../../commands/registry";
import {
  MENU_COMMANDS,
  MENU_SECTIONS,
  NATIVE_MENU_CHORDS,
  fixedMenuChords,
  menuCommandsFor,
} from "../../commands/menu-commands";
import { setKeybindingOverrides } from "../../commands/keybindings";
import { formatKeybinding } from "../../lib/keybinding-format";
import { EDITOR_COMMAND_KEYS } from "../../editor/editor-command-keys";
import { appMenuItems } from "../../components/TitleBar/AppMenu";
import { APP_REGISTRATIONS, registrationsIn, type Registration } from "./app-registrations";
import { TRANSFORM_SUBMENU } from "../../commands/transforms";

// The shared list is the contract: the macOS menu bar is built from it in
// `src-tauri/src/menu.rs`, and `AppMenu.tsx` builds the Windows and Linux menu
// from the same entries. These tests hold the frontend half of that; the Rust
// half is `menu::tests`.

/**
 * The registration behind a menu entry. This fails rather than skipping: an id
 * with no `registerCommand` renders nothing on Windows and Linux and routes
 * nowhere from the macOS menu, which is exactly what these tests are for.
 */
const PROMPT_REGISTRATIONS = registrationsIn("src/commands/prompt.ts");

function registrationFor(id: string): Registration {
  const registration = APP_REGISTRATIONS.get(id) ?? PROMPT_REGISTRATIONS.get(id);
  expect(registration, `${id} is in the menu with no registerCommand in App.tsx`).toBeDefined();
  return registration!;
}

/**
 * The transforms register from the Rust registry at runtime, one
 * `transform.<id>` per built-in; `menu::tests` holds the submenu's ids and
 * words to that registry. Every other entry names a literal registration.
 */
const isTransform = (entry: { submenu?: string }) => entry.submenu === TRANSFORM_SUBMENU;
const LITERAL_ENTRIES = MENU_COMMANDS.filter((entry) => !isTransform(entry));

/** Menu wording and palette wording differ in case and in the trailing "…". */
function sameWords(menuLabel: string, registryLabel: string): boolean {
  const normalize = (text: string) => text.replace(/…$/, "").trim().toLowerCase();
  const registry = normalize(registryLabel);
  // A label that names the platform's file manager is a template; the word it
  // interpolates is the host's, and the menu's own copy says "Finder".
  if (registry.includes("${")) {
    const pattern = new RegExp(
      `^${registry.replace(/[.*+?^${}()|[\]\\]/g, "\\$&").replace(/\\\$\\\{[^}]*\\\}/, ".+")}$`,
    );
    return pattern.test(normalize(menuLabel));
  }
  return normalize(menuLabel) === registry;
}

afterEach(() => {
  for (const entry of MENU_COMMANDS) unregisterCommand(entry.id);
  setKeybindingOverrides({});
});

function registerEveryMenuCommand(ran: string[]): void {
  for (const entry of MENU_COMMANDS) {
    registerCommand({
      id: entry.id,
      label: entry.label,
      keybinding: entry.accelerator,
      scope: "app",
      execute: () => {
        ran.push(entry.id);
      },
    });
  }
}

describe("the shared menu command list", () => {
  it("offers every command on every platform, so nothing is macOS-only", () => {
    const mac = menuCommandsFor("mac").map((entry) => entry.id);
    expect(menuCommandsFor("win").map((entry) => entry.id)).toEqual(mac);
    expect(menuCommandsFor("linux").map((entry) => entry.id)).toEqual(mac);
    expect(mac.length).toBe(MENU_COMMANDS.length);
  });

  it("names a menu every entry can hang under", () => {
    for (const entry of MENU_COMMANDS) {
      expect(MENU_SECTIONS, entry.id).toContain(entry.menu);
    }
  });

  it("claims no accelerator twice", () => {
    const accelerators = MENU_COMMANDS.map((entry) => entry.accelerator).filter(Boolean);
    expect(accelerators).toEqual([...new Set(accelerators)]);
  });

  it("carries the File and View items the menu bar is expected to have", () => {
    const ids = MENU_COMMANDS.map((entry) => entry.id);
    for (const id of [
      "history.openRecent",
      "notes.showFolder",
      "buffer.save",
      "note.today",
      "settings.open",
      "app.thirdPartyNotices",
      "editor.find",
      "editor.findNext",
      "editor.findPrevious",
      "editor.replace",
      "sidebar.toggle",
      "panel.toggle",
      "folderGraph.open",
    ]) {
      expect(ids, id).toContain(id);
    }
    expect(MENU_COMMANDS.some((entry) => entry.menu === "help")).toBe(true);
  });

  it("opens the folder graph from the View menu on every platform", () => {
    for (const platform of ["mac", "win", "linux"] as const) {
      const view = menuCommandsFor(platform).filter((entry) => entry.menu === "view");
      expect(
        view.map((entry) => entry.id),
        `the View menu on ${platform}`,
      ).toContain("folderGraph.open");
    }
  });

  it("names only commands App.tsx registers", () => {
    for (const entry of LITERAL_ENTRIES) registrationFor(entry.id);
  });

  it("names a transform the transform registration makes, in one submenu", () => {
    const transforms = MENU_COMMANDS.filter(isTransform);
    expect(transforms).toHaveLength(9);
    for (const entry of transforms) {
      expect(entry.id, entry.id).toMatch(/^transform\.[a-z_]+$/);
      expect(entry.menu).toBe("edit");
    }
    const first = MENU_COMMANDS.indexOf(transforms[0]);
    expect(MENU_COMMANDS.slice(first, first + transforms.length)).toEqual(transforms);
  });

  it("puts Search Everywhere, Open Folder and Copy as Prompt where a person looks", () => {
    const where = (id: string) => MENU_COMMANDS.find((entry) => entry.id === id);
    expect(where("search.openEverywhere")?.menu).toBe("edit");
    expect(where("search.openEverywhere")?.accelerator).toBe("CmdOrCtrl+Shift+F");
    expect(where("workspace.openFolder")?.menu).toBe("file");
    expect(where("workspace.openFolder")?.accelerator).toBe("CmdOrCtrl+Alt+O");
    expect(where("prompt.copyAsPrompt")?.menu).toBe("edit");
  });

  it("gives Help the palette and the five help items", () => {
    expect(
      MENU_COMMANDS.filter((entry) => entry.menu === "help").map((entry) => entry.label),
    ).toEqual([
      "Command Palette…",
      "Writ Help",
      "Keyboard Shortcuts",
      "What's New",
      "Show Setup Again",
      "Report an Issue",
    ]);
  });

  it("gives each entry the chord its command is registered with", () => {
    for (const entry of LITERAL_ENTRIES) {
      const registration = registrationFor(entry.id);
      if (!entry.accelerator) continue;
      expect(registration.keybinding, entry.id).toBe(entry.accelerator);
    }
  });

  it("words each entry the way its command is worded", () => {
    for (const entry of LITERAL_ENTRIES) {
      const registration = registrationFor(entry.id);
      expect(registration.label, `${entry.id} is registered with no label`).not.toBe("");
      expect(
        sameWords(entry.label, registration.label),
        `${entry.id}: menu says "${entry.label}", the command says "${registration.label}"`,
      ).toBe(true);
    }
  });
});

describe("AppMenu", () => {
  it("renders the same command set as the macOS menu, in the list's order", () => {
    const ran: string[] = [];
    registerEveryMenuCommand(ran);

    const labels = appMenuItems().map((item) => item.label);
    expect(labels).toEqual(MENU_COMMANDS.map((entry) => entry.label));
  });

  it("runs the command an entry names", async () => {
    const ran: string[] = [];
    registerEveryMenuCommand(ran);

    const item = appMenuItems().find((entry) => entry.label === "Open Recent");
    expect(item).toBeDefined();
    item!.action();
    await Promise.resolve();

    expect(ran).toEqual(["history.openRecent"]);
  });

  it("divides the menus from each other", () => {
    const ran: string[] = [];
    registerEveryMenuCommand(ran);

    const items = appMenuItems();
    expect(items[0].separator).toBeFalsy();
    const dividedLabels = items.filter((item) => item.separator).map((item) => item.label);
    // The first item of each menu after the app menu opens a run.
    expect(dividedLabels).toContain("New File");
    expect(dividedLabels).toContain("Find");
    expect(dividedLabels).toContain("Toggle Sidebar");
  });

  it("shows the shortcut a command carries", () => {
    const ran: string[] = [];
    registerEveryMenuCommand(ran);

    const save = appMenuItems().find((item) => item.label === "Save");
    expect(save?.kbd).toBeTruthy();
  });

  it("shows the chord a command was rebound to, not the shipped one", () => {
    const ran: string[] = [];
    registerEveryMenuCommand(ran);
    setKeybindingOverrides({ "sidebar.toggle": "CmdOrCtrl+Shift+B" });

    const sidebar = appMenuItems().find((item) => item.label === "Toggle Sidebar");
    expect(sidebar?.kbd).toBe(formatKeybinding("CmdOrCtrl+Shift+B"));
  });

  it("heads the transforms with the name the macOS bar gives their submenu", () => {
    const ran: string[] = [];
    registerEveryMenuCommand(ran);

    const items = appMenuItems();
    const headed = items.filter((item) => item.heading !== undefined);
    expect(headed.map((item) => item.heading)).toEqual(["Transform"]);
    expect(headed[0].label).toBe("Trim Trailing Spaces");
    expect(headed[0].separator).toBe(true);
    const at = items.indexOf(headed[0]);
    expect(items[at + 9].label).toBe("Copy as Prompt");
  });

  it("lists a transform by its menu label, without the palette's prefix", () => {
    registerCommand({
      id: "transform.dedent",
      label: "Transform: Remove shared indentation",
      menuLabel: "Remove shared indentation",
      scope: "app",
      execute: () => {},
    });

    const labels = appMenuItems().map((item) => item.label);
    expect(labels).toContain("Remove shared indentation");
    expect(labels).not.toContain("Transform: Remove shared indentation");
  });

  it("leaves an unregistered id out rather than rendering an empty row", () => {
    const ran: string[] = [];
    registerEveryMenuCommand(ran);
    unregisterCommand("buffer.save");

    expect(appMenuItems().some((item) => item.label === "Save")).toBe(false);
  });
});

describe("AppMenu on Windows and Linux", () => {
  it("is the whole menu there: every command is reachable from it", () => {
    const ran: string[] = [];
    registerEveryMenuCommand(ran);

    // `appMenuItems` reads the host platform, which is what the test runs on,
    // and the list is platform-complete, so the ids it yields are the ids the
    // Windows and Linux menus yield.
    const rendered = appMenuItems().map((item) => item.label);
    for (const entry of menuCommandsFor("win")) expect(rendered, entry.id).toContain(entry.label);
    for (const entry of menuCommandsFor("linux")) expect(rendered, entry.id).toContain(entry.label);
  });

});

describe("the macOS menu bar's own chords", () => {
  it("are taken on macOS only", () => {
    expect(fixedMenuChords("mac").map((item) => item.chord)).toContain("CmdOrCtrl+Q");
    expect(fixedMenuChords("win")).toEqual([]);
    expect(fixedMenuChords("linux")).toEqual([]);
  });

  it("are claimed by no command, editor chord or menu item", () => {
    const native = new Set(NATIVE_MENU_CHORDS.map((item) => item.chord));
    const claimed = [
      ...[...APP_REGISTRATIONS.values()].flatMap((r) => [r.keybinding, ...r.aliases]),
      ...EDITOR_COMMAND_KEYS.flatMap((key) => [key.keybinding, ...(key.aliases ?? [])]),
      ...MENU_COMMANDS.map((entry) => entry.accelerator ?? ""),
    ].filter((chord) => native.has(chord));
    expect(claimed).toEqual([]);
  });
});
