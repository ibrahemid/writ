import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// The editor's own chrome: the status bar, the find bar, the palette and the
// preview controls. jsdom does no layout and resolves no custom property, so
// these read the sheets as text and pin the property each rule is required to
// spend.

const read = (path: string) => readFileSync(resolve(process.cwd(), path), "utf8");

const SHEETS = {
  statusbar: "src/components/Editor/StatusBar.css",
  find: "src/components/Find/FindOverlay.css",
  palette: "src/components/Palette/Palette.css",
  layoutToggle: "src/components/Preview/preview-layout-toggle.css",
} as const;

const CSS: Record<keyof typeof SHEETS, string> = {
  statusbar: read(SHEETS.statusbar),
  find: read(SHEETS.find),
  palette: read(SHEETS.palette),
  layoutToggle: read(SHEETS.layoutToggle),
};

/** The declaration body of one rule, matched on its own selector line. */
function rule(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = css.match(new RegExp(`^${escaped}\\s*\\{([^}]*)\\}`, "m"));
  expect(match, `${selector} is declared`).toBeTruthy();
  return match![1]!;
}

// --writ-fg-faint is 3.7:1 on the canvas in light and 4.2:1 in dark, under AA
// for the 12px these fields are set in, and the baseline reserves it for hung
// markers and placeholders. A field that carries data reads as data.
describe("the chrome's informational fields", () => {
  const INFORMATIONAL: [keyof typeof SHEETS, string][] = [
    ["statusbar", ".statusbar-field"],
    ["statusbar", ".statusbar-tokens"],
    ["find", ".find-count"],
    ["palette", ".palette-item-desc"],
    ["layoutToggle", ".layout-toggle-seg"],
    ["layoutToggle", ".scripts-toggle.is-off"],
  ];

  for (const [sheet, selector] of INFORMATIONAL) {
    it(`paints ${selector} in the muted foreground`, () => {
      expect(rule(CSS[sheet], selector)).toMatch(/color:\s*var\(--writ-fg-muted\)/);
    });
  }

  it("keeps the faint foreground for placeholders", () => {
    expect(rule(CSS.find, ".find-input::placeholder")).toMatch(
      /color:\s*var\(--writ-fg-faint\)/,
    );
    expect(rule(CSS.palette, ".palette-input::placeholder")).toMatch(
      /color:\s*var\(--writ-fg-faint\)/,
    );
  });
});
