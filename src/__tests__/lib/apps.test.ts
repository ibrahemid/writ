import { describe, it, expect } from "vitest";
import { APPS, OFFERED_BY_FILE, OPENS_ON_FIRST_SWITCH, appEntry, fileShowsUseFor } from "../../lib/apps";
import { ICON_NAMES } from "../../components/Icon/sprite.generated";
import { MENU_COMMANDS } from "../../commands/menu-commands";
import { APP_REGISTRATIONS } from "../commands/app-registrations";
import { entryPointName } from "../../components/Apps/AppEntryPoints";

// The contract lib/apps.ts carries for every app: an icon, and where the app
// lives once it is on, each place naming something that is really there.

describe("every app", () => {
  it("has an icon the sprite draws", () => {
    for (const app of APPS) expect(ICON_NAMES, app.id).toContain(app.icon);
  });

  it("names at least one place it lives", () => {
    for (const app of APPS) expect(app.entryPoints.length, app.id).toBeGreaterThan(0);
  });

  it("names a menu item the shared menu list carries for that app", () => {
    for (const app of APPS) {
      for (const point of app.entryPoints) {
        if (point.kind !== "menu") continue;
        const entry = MENU_COMMANDS.find((item) => item.id === point.command);
        expect(entry, `${app.id}: ${point.command}`).toBeDefined();
        expect(entry!.app, point.command).toBe(app.id);
      }
    }
  });

  it("names a chord App.tsx gives the command", () => {
    for (const app of APPS) {
      for (const point of app.entryPoints) {
        if (point.kind !== "chord") continue;
        expect(APP_REGISTRATIONS.get(point.command)?.keybinding, point.command).not.toBe("");
      }
    }
  });

  it("names the toolbar only for an app the toolbar draws a button for", () => {
    const withButton = APPS.filter((app) => app.entryPoints.some((p) => p.kind === "toolbar"));
    expect(withButton.map((app) => app.id)).toEqual(["chat", "programs", "connections", "graph"]);
  });
});

describe("where an app lives, in words", () => {
  it("names the macOS menu the item is in, and the one menu elsewhere", () => {
    const point = { kind: "menu", command: "folderGraph.open" } as const;
    expect(entryPointName(point, "mac")).toBe("View menu");
    expect(entryPointName(point, "win")).toBe("Writ menu");
    expect(entryPointName(point, "linux")).toBe("Writ menu");
  });

  it("draws a chord as keys rather than naming it", () => {
    expect(entryPointName({ kind: "chord", command: "panel.toggle" }, "mac")).toBeNull();
    expect(entryPointName({ kind: "toolbar" }, "win")).toBe("Toolbar");
  });
});

describe("offers", () => {
  it("are made for the apps a file can show a use for, in that order", () => {
    expect(OFFERED_BY_FILE).toEqual(["connections", "tags"]);
    for (const app of OFFERED_BY_FILE) expect(appEntry(app).offer).toBeTruthy();
  });

  it("read links for Connections and tags for Tags, and nothing for the rest", () => {
    const none = { links: [], tags: [] };
    expect(fileShowsUseFor("connections", { links: [1], tags: [] })).toBe(true);
    expect(fileShowsUseFor("connections", none)).toBe(false);
    expect(fileShowsUseFor("tags", { links: [], tags: [1] })).toBe(true);
    expect(fileShowsUseFor("tags", { links: [1], tags: [] })).toBe(false);
    expect(fileShowsUseFor("graph", { links: [1], tags: [1] })).toBe(false);
  });
});

describe("the first switch-on", () => {
  it("opens the apps with a surface in the window, not Rewrite or programs", () => {
    expect([...OPENS_ON_FIRST_SWITCH].sort()).toEqual(["chat", "connections", "graph", "tags"]);
  });
});
