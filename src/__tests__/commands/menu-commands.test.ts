import { describe, it, expect, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { registerCommand, unregisterCommand } from "../../commands/registry";
import { MENU_COMMANDS, MENU_SECTIONS, menuCommandsFor } from "../../commands/menu-commands";
import { appMenuItems } from "../../components/TitleBar/AppMenu";

// The shared list is the contract: the macOS menu bar is built from it in
// `src-tauri/src/menu.rs`, and `AppMenu.tsx` builds the Windows and Linux menu
// from the same entries. These tests hold the frontend half of that; the Rust
// half is `menu::tests`.

const APP_TSX = readFileSync(resolve(process.cwd(), "src/App.tsx"), "utf8");

/** The object literal passed to a `registerCommand({ … })` call. */
function objectLiteral(text: string, open: number): string {
  let depth = 0;
  for (let i = open; i < text.length; i += 1) {
    if (text[i] === "{") depth += 1;
    else if (text[i] === "}") {
      depth -= 1;
      if (depth === 0) return text.slice(open + 1, i);
    }
  }
  throw new Error("unterminated registerCommand literal");
}

function field(literal: string, name: string): string {
  const match = literal.match(new RegExp(`(^|[\\s{,])${name}:\\s*([^\\n]+?),?\\s*$`, "m"));
  return match ? match[2].trim() : "";
}

/** Strips the quotes, and the escaping a TypeScript literal needs. */
function literalText(raw: string): string {
  return raw.replace(/^["'`]|["'`]$/g, "").replace(/\\\\/g, "\\");
}

interface Registration {
  label: string;
  keybinding: string;
}

/** Every command `App.tsx` registers, by id, as written in the file. */
function registrations(): Map<string, Registration> {
  const found = new Map<string, Registration>();
  const marker = "registerCommand({";
  let at = APP_TSX.indexOf(marker);
  while (at !== -1) {
    const literal = objectLiteral(APP_TSX, at + marker.length - 1);
    const id = literalText(field(literal, "id"));
    if (id) {
      found.set(id, {
        label: literalText(field(literal, "label")),
        keybinding: literalText(field(literal, "keybinding")),
      });
    }
    at = APP_TSX.indexOf(marker, at + marker.length);
  }
  return found;
}

const REGISTRATIONS = registrations();

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
    ]) {
      expect(ids, id).toContain(id);
    }
    expect(MENU_COMMANDS.some((entry) => entry.menu === "help")).toBe(true);
  });

  it("gives each entry the chord its command is registered with", () => {
    for (const entry of MENU_COMMANDS) {
      const registration = REGISTRATIONS.get(entry.id);
      if (!registration) continue;
      if (!entry.accelerator) continue;
      expect(registration.keybinding, entry.id).toBe(entry.accelerator);
    }
  });

  it("words each entry the way its command is worded", () => {
    for (const entry of MENU_COMMANDS) {
      const registration = REGISTRATIONS.get(entry.id);
      if (!registration || !registration.label) continue;
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
    expect(dividedLabels).toContain("New Note");
    expect(dividedLabels).toContain("Find");
    expect(dividedLabels).toContain("Toggle Sidebar");
  });

  it("shows the shortcut a command carries", () => {
    const ran: string[] = [];
    registerEveryMenuCommand(ran);

    const save = appMenuItems().find((item) => item.label === "Save");
    expect(save?.kbd).toBeTruthy();
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
