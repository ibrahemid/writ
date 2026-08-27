import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { themeStore } from "../../stores/global/theme";

// Two fixtures, both captured from origin/main before the pipeline existed: the
// property map the theme store writes, and the :root block the hand-written
// stylesheet declared. Moving both onto design/tokens must not move a value.

const ROOT = process.cwd();

const RESOLVED_FIXTURE = JSON.parse(
  readFileSync(resolve(ROOT, "src/__tests__/fixtures/resolved-tokens-warp-dark.json"), "utf8"),
) as Record<string, unknown>;

const ORIGIN_ROOT = JSON.parse(
  readFileSync(resolve(ROOT, "src/__tests__/fixtures/root-tokens-origin-main.json"), "utf8"),
) as Record<string, string>;

const THEME_CSS = readFileSync(resolve(ROOT, "src/styles/generated/theme.css"), "utf8");
const LEGACY_CSS = readFileSync(resolve(ROOT, "src/styles/generated/legacy-aliases.css"), "utf8");

function declarations(css: string): Map<string, string> {
  const out = new Map<string, string>();
  for (const line of css.split("\n")) {
    const m = /^\s*(--[a-z0-9-]+)\s*:\s*(.+);\s*$/.exec(line);
    if (m && !out.has(m[1])) out.set(m[1], m[2]);
  }
  return out;
}

const LEGACY = declarations(LEGACY_CSS);
const GENERATED = declarations(THEME_CSS);

// The names the legacy layer freezes: everything the old sheet declared that is
// not a colour the theme store overwrites from the active preset.
const FROZEN = [
  "--writ-font-sans",
  "--writ-font-size",
  "--writ-font-size-sm",
  "--writ-font-size-xs",
  "--writ-line-height",
  "--writ-editor-font-size",
  "--writ-radius-1",
  "--writ-radius-2",
  "--writ-radius-3",
  "--writ-window-radius",
  "--writ-sidebar-width",
  "--writ-statusbar-height",
  "--writ-tabbar-height",
  "--writ-titlebar-height",
  "--writ-tab-pill-height",
  "--writ-bg-tab-pill",
  "--writ-overlay-hover",
  "--writ-overlay-subtle",
  "--writ-overlay-scrim",
  "--writ-selection",
  "--writ-warning-foreground",
  "--writ-shadow-modal",
  "--writ-shadow-overlay",
  "--writ-shadow-dialog",
  "--writ-shadow-popover",
  "--writ-shadow-banner",
  "--writ-shadow-toast",
  "--writ-shadow-sidebar",
  "--writ-shadow-chip",
  "--writ-shadow-xs",
];

describe("token pipeline acceptance", () => {
  it("resolved token set is unchanged by the pipeline", () => {
    themeStore.resetOverrides();
    themeStore.setPreset("warp-dark");
    themeStore.applyToRoot(document.createElement("div"));
    const written = JSON.parse(localStorage.getItem("writ-theme-vars") as string) as Record<
      string,
      unknown
    >;
    expect(Object.keys(written).sort()).toEqual(Object.keys(RESOLVED_FIXTURE).sort());
    expect(written).toEqual(RESOLVED_FIXTURE);
  });

  it("chrome metrics and elevation keep the values origin/main declared", () => {
    for (const name of FROZEN) {
      expect(LEGACY.get(name), `${name} drifted`).toBe(ORIGIN_ROOT[name]);
    }
  });

  it("every custom property origin/main declared is still declared", () => {
    const missing = Object.keys(ORIGIN_ROOT).filter(
      (name) => !LEGACY.has(name) && !GENERATED.has(name),
    );
    expect(missing, `dropped: ${missing.join(", ")}`).toEqual([]);
  });

  it("the re-declared elevation names outrank the dark layer", () => {
    // Same specificity as :root[data-theme="dark"], and the legacy sheet is
    // imported after the generated one, so source order settles it.
    expect(LEGACY_CSS).toContain(":root,\n:root[data-theme] {");
    for (const name of ["--writ-shadow-modal", "--writ-shadow-popover", "--writ-shadow-chip"]) {
      expect(GENERATED.has(name), `${name} should exist in both layers`).toBe(true);
      expect(LEGACY.has(name), `${name} should be re-declared`).toBe(true);
    }
  });
});
