import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// The chrome reads --writ-ui-*, so raising the interface text size to 22px
// raises every label with it. These tests pin the rules that decide what a
// strip, a bar and a row do when the text no longer fits: truncate, keep one
// line, and grow the box rather than cut the control off.
//
// jsdom does no layout, so the assertions are on the rules themselves. The
// rendered result at 12, 16 and 22 is a design-gate pass, not a unit test.

const read = (path: string) => readFileSync(resolve(process.cwd(), path), "utf8");

const TABBAR = read("src/components/Editor/TabBar.css");
const STATUSBAR = read("src/components/Editor/StatusBar.css");
const SETTINGS = read("src/components/SettingsModal/SettingsModal.css");
const BUTTON = read("src/components/Button/Button.css");
const SIDEBAR = read("src/components/Sidebar/Sidebar.css");
const TITLEBAR = read("src/components/TitleBar/TitleBar.css");
const CONTEXTMENU = read("src/components/ContextMenu/ContextMenu.css");
const SEARCHBAR = read("src/components/Sidebar/SearchBar.css");
const TOOLBAR = read("src/components/Toolbar/Toolbar.css");
const FIND = read("src/components/Find/FindOverlay.css");
const PALETTE = read("src/components/Palette/Palette.css");

/** The declaration body of one rule, matched on its own selector line. */
function rule(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = css.match(new RegExp(`^${escaped}\\s*\\{([^}]*)\\}`, "m"));
  expect(match, `${selector} is declared`).toBeTruthy();
  return match![1];
}

function declares(css: string, selector: string, property: string, value: RegExp): void {
  expect(rule(css, selector), `${selector} { ${property} }`).toMatch(
    new RegExp(`${property}:\\s*${value.source}`),
  );
}

/** A box whose text comes from a --writ-ui-* step must not be a fixed height. */
function isNotFixedHeight(css: string, selector: string): void {
  expect(rule(css, selector), `${selector} sets a fixed height`).not.toMatch(
    /(^|[\s;])height:\s*(\d|var\()/,
  );
}

describe("the tab strip at the top of the interface text range", () => {
  it("truncates a title rather than letting it overflow the tab", () => {
    declares(TABBAR, ".tab-title", "text-overflow", /ellipsis/);
    declares(TABBAR, ".tab-title", "white-space", /nowrap/);
    declares(TABBAR, ".tab-title", "overflow", /hidden/);
    declares(TABBAR, ".tab-label", "overflow", /hidden/);
    declares(TABBAR, ".tab-label", "min-width", /0/);
  });

  it("caps the tab and lets the strip scroll instead of stretching the bar", () => {
    declares(TABBAR, ".tab", "max-width", /\d+px/);
    declares(TABBAR, ".tabbar-tabs", "min-width", /0/);
    declares(TABBAR, ".tabbar-tabs", "overflow-x", /auto/);
  });

  it("treats the platform tab metrics as floors, so a tall label is not cut off", () => {
    for (const selector of [
      ".tabbar",
      ".tab",
      ".tab-add",
      '.tabbar[data-platform="linux"]',
      '.tabbar[data-platform="linux"] .tab',
      '.tabbar[data-platform="linux"] .tab-add',
    ]) {
      isNotFixedHeight(TABBAR, selector);
      declares(TABBAR, selector, "min-height", /\d+px/);
    }
  });
});

describe("the status bar at the top of the interface text range", () => {
  it("keeps every field on one line", () => {
    declares(STATUSBAR, ".statusbar", "white-space", /nowrap/);
    expect(rule(STATUSBAR, ".statusbar")).not.toMatch(/flex-wrap:\s*wrap/);
  });

  it("grows rather than clipping when the line box passes the bar metric", () => {
    isNotFixedHeight(STATUSBAR, ".statusbar");
    declares(STATUSBAR, ".statusbar", "min-height", /var\(--writ-statusbar-height\)/);
  });

  it("sheds its fields on a text-relative width, not a pixel one", () => {
    const conditions = Array.from(STATUSBAR.matchAll(/@container \(max-width: ([^)]+)\)/g)).map(
      (m) => m[1],
    );
    expect(conditions.length).toBeGreaterThan(0);
    for (const condition of conditions) expect(condition).toMatch(/^[\d.]+em$/);
  });
});

describe("a settings row at the top of the interface text range", () => {
  it("yields the label rather than pushing the control out of the row", () => {
    declares(SETTINGS, ".settings-row-label", "min-width", /0/);
    declares(SETTINGS, ".settings-input-number", "flex-shrink", /0/);
    declares(SETTINGS, ".settings-select", "flex-shrink", /0/);
  });

  it("treats control heights as floors, so a taller label is not clipped", () => {
    for (const selector of [".settings-input", ".settings-select", ".settings-seg-option"]) {
      isNotFixedHeight(SETTINGS, selector);
      declares(SETTINGS, selector, "min-height", /\d+px/);
    }
  });
});

describe("the sidebar and the chrome menus at the top of the interface text range", () => {
  it("treats the row pitch as a floor, so a descender is not clipped", () => {
    isNotFixedHeight(SIDEBAR, ".sidebar-row");
    declares(SIDEBAR, ".sidebar-row", "min-height", /var\(--writ-sidebar-row-fill\)/);
  });

  it("treats the app menu, a menu item and the search field as floors too", () => {
    for (const [css, selector] of [
      [TITLEBAR, ".titlebar-appmenu"],
      [CONTEXTMENU, ".context-menu-item"],
      [SEARCHBAR, ".search-field"],
      [TOOLBAR, '.writ-toolbar[data-platform="win"] .search-field'],
    ] as const) {
      isNotFixedHeight(css, selector);
      declares(css, selector, "min-height", /\d+px/);
    }
  });
});

describe("a button at the top of the interface text range", () => {
  it("treats its platform height as a floor on all three shells", () => {
    for (const selector of [
      ".writ-btn",
      ':root[data-platform="win"] .writ-btn',
      ':root[data-platform="linux"] .writ-btn',
    ]) {
      isNotFixedHeight(BUTTON, selector);
      declares(BUTTON, selector, "min-height", /\d+px/);
    }
  });
});

describe("the find bar and the palette at the top of the interface text range", () => {
  it("treats the find bar's box sizes as floors", () => {
    isNotFixedHeight(FIND, ".find-input");
    declares(FIND, ".find-input", "min-height", /\d+px/);
    // The buttons are the shared control; their find-bar metrics are written at
    // a weight that beats Button.css, and they stay floors there too.
    const controls =
      ":root .find-overlay .find-row .find-toggle,\n:root .find-overlay .find-row .find-icon-btn";
    isNotFixedHeight(FIND, controls);
    declares(FIND, controls, "min-height", /\d+px/);
  });

  it("treats the palette's input and row heights as floors", () => {
    for (const selector of [".palette-search", ".palette-item"]) {
      isNotFixedHeight(PALETTE, selector);
      declares(PALETTE, selector, "min-height", /\d+px/);
    }
  });
});
