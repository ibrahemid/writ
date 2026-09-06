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
    /(^|[\s;])height:\s*\d/,
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
