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
  firstRun: "src/components/Editor/FirstRunHint.css",
  linkPicker: "src/components/Editor/LinkAmbiguityPicker.css",
  spellingPreview: "src/components/Editor/SpellingPreview.css",
  find: "src/components/Find/FindOverlay.css",
  palette: "src/components/Palette/Palette.css",
  layoutToggle: "src/components/Preview/preview-layout-toggle.css",
  previewChrome: "src/components/Preview/preview-chrome.css",
  linkConfirm: "src/components/Preview/LinkConfirm.css",
} as const;

const CSS = Object.fromEntries(
  Object.entries(SHEETS).map(([key, path]) => [key, read(path)]),
) as Record<keyof typeof SHEETS, string>;

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

// focus.css moves the ring per shell: Windows gets an inverted outer ring at
// offset 1, GNOME a half-strength accent inside the control. A hand-written
// ring applies the macOS geometry everywhere, and a control that cancels the
// ring in CSS has no way back — the silent attribute is the one opt-out, and it
// is on the element, where the reason for the silence is visible.
describe("the editor chrome's focus rings", () => {
  it("draws every ring from the focus properties", () => {
    for (const [name, css] of Object.entries(CSS)) {
      const rings = [...css.matchAll(/outline:\s*([^;]+);/g)].map((m) => m[1].trim());
      for (const ring of rings) {
        expect(ring, `${name} draws ${ring}`).toMatch(/^var\(--writ-focus-outline[,)]/);
      }
    }
  });

  it("offsets every ring from the focus property", () => {
    for (const [name, css] of Object.entries(CSS)) {
      const offsets = [...css.matchAll(/outline-offset:\s*([^;]+);/g)].map((m) => m[1].trim());
      for (const offset of offsets) {
        expect(offset, `${name} offsets by ${offset}`).toMatch(/^var\(--writ-focus-offset[,)]/);
      }
    }
  });
});
