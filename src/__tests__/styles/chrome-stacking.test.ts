import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// The find panel must outrank the word count (--writ-z-chrome) so its close
// button stays clickable, and the compose label must never wrap at a narrow
// window width.

const FIND_CSS = readFileSync(
  resolve(process.cwd(), "src/components/Find/FindOverlay.css"),
  "utf8",
);
const TOOLBAR_CSS = readFileSync(
  resolve(process.cwd(), "src/components/Toolbar/Toolbar.css"),
  "utf8",
);
const TITLEBAR_CSS = readFileSync(
  resolve(process.cwd(), "src/components/TitleBar/TitleBar.css"),
  "utf8",
);
const PICKER_CSS = readFileSync(
  resolve(process.cwd(), "src/components/Editor/LinkAmbiguityPicker.css"),
  "utf8",
);
const SPELLING_CSS = readFileSync(
  resolve(process.cwd(), "src/components/Editor/SpellingPreview.css"),
  "utf8",
);
const THEME_CSS = readFileSync(resolve(process.cwd(), "src/styles/generated/theme.css"), "utf8");

const layer = (name: string): number => {
  const m = THEME_CSS.match(new RegExp(`--writ-z-${name}:\\s*(\\d+)`));
  expect(m, `--writ-z-${name} is declared`).toBeTruthy();
  return Number(m![1]);
};

const zToken = (css: string, selector: string): string => {
  const m = block(css, selector).match(/z-index:\s*var\((--writ-z-[a-z-]+)\)/);
  expect(m, `${selector} spends a z-index token`).toBeTruthy();
  return m![1]!;
};

const block = (css: string, selector: string): string => {
  const m = css.match(new RegExp(`${selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*\\{([^}]*)\\}`));
  expect(m, `${selector} is declared`).toBeTruthy();
  return m![1]!;
};

describe("find panel stacking", () => {
  it("reads the popover z-index token, not a raw number", () => {
    expect(block(FIND_CSS, ".find-overlay")).toMatch(/z-index:\s*var\(--writ-z-popover\)/);
  });

  it("keeps the tickmap on the same popover layer", () => {
    expect(block(FIND_CSS, ".find-tickmap")).toMatch(/z-index:\s*var\(--writ-z-popover\)/);
  });

  it("never hardcodes a z-index number", () => {
    expect(FIND_CSS).not.toMatch(/z-index:\s*\d/);
  });
});

// Nothing in the chain from the root to the picker opens a stacking context, so
// a number below the layer tokens puts an aria-modal dialog under the find
// panel, the toolbar and the toasts.
describe("link picker stacking", () => {
  it("sits on the modal layer, above every chrome layer", () => {
    expect(zToken(PICKER_CSS, ".link-picker-scrim")).toBe("--writ-z-modal");
    expect(layer("modal")).toBeGreaterThan(layer("popover"));
  });

  it("never hardcodes a z-index number", () => {
    expect(PICKER_CSS).not.toMatch(/z-index:\s*\d/);
  });
});

// A number above the popover layer means nothing: the context menu is on that
// layer and mounts after the editor, so it still paints over the panel.
describe("spelling panel stacking", () => {
  it("sits on the popover layer, by name", () => {
    expect(zToken(SPELLING_CSS, ".spelling-preview")).toBe("--writ-z-popover");
    expect(SPELLING_CSS).not.toMatch(/z-index:\s*\d/);
  });
});

// The lights overlap the toolbar, so their order cannot rest on DOM position:
// a reorder of `.app-body` would put the toolbar's opaque background over them.
describe("window lights stacking", () => {
  it("puts the lights layer on a named layer above the toolbar's", () => {
    const lights = zToken(TITLEBAR_CSS, ".window-lights-layer");
    const toolbar = zToken(TOOLBAR_CSS, ".writ-toolbar");
    expect(lights).toBe("--writ-z-window-lights");
    expect(toolbar).toBe("--writ-z-chrome");
    expect(layer("window-lights")).toBeGreaterThan(layer("chrome"));
  });

  it("keeps that layer under the overlays", () => {
    expect(layer("window-lights")).toBeLessThan(layer("popover"));
  });
});

describe("toolbar compose label", () => {
  it("never wraps to a second line", () => {
    const compose = block(TOOLBAR_CSS, ".writ-toolbar-compose");
    expect(compose).toMatch(/white-space:\s*nowrap/);
    expect(compose).toMatch(/flex:\s*none/);
  });

  it("lets the search field shrink before the compose label does", () => {
    const searchBlock = block(TOOLBAR_CSS, ".writ-toolbar .search-bar");
    expect(searchBlock).toMatch(/flex:\s*0 1 180px/);
    expect(searchBlock).toMatch(/min-width:\s*120px/);
  });
});
