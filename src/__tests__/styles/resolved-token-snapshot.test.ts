import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { themeStore } from "../../stores/global/theme";
import { ACCENTS } from "../../styles/generated/tokens";

// Two fixtures, both captured from origin/main before the pipeline existed: the
// property map the theme store writes, and the :root block the hand-written
// stylesheet declared. The pipeline moved them without moving a value; the
// visual flip then moves the ones listed in REPAINTED, on purpose. Every other
// name still has to resolve to what 0.3.5 painted, so an unmigrated stylesheet
// keeps rendering as it did.

const ROOT = process.cwd();

const RESOLVED_FIXTURE = JSON.parse(
  readFileSync(resolve(ROOT, "src/__tests__/fixtures/resolved-tokens-warp-dark.json"), "utf8"),
) as Record<string, string>;

const ORIGIN_ROOT = JSON.parse(
  readFileSync(resolve(ROOT, "src/__tests__/fixtures/root-tokens-origin-main.json"), "utf8"),
) as Record<string, string>;

// global.css imports the generated sheet first and the legacy layer second, so
// a name both declare resolves to the legacy declaration at equal specificity.
const SHEETS = [
  "src/styles/generated/theme.css",
  "src/styles/generated/legacy-aliases.css",
].map((file) => readFileSync(resolve(ROOT, file), "utf8"));

const LEGACY_CSS = SHEETS[1];

interface Rule {
  selector: string;
  declarations: Map<string, string>;
  order: number;
}

function parseRules(css: string, order: number): Rule[] {
  const rules: Rule[] = [];
  const stripped = css.replace(/\/\*[\s\S]*?\*\//g, "");
  for (const [, selectorList, body] of stripped.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    if (selectorList.includes("@")) continue;
    const declarations = new Map<string, string>();
    for (const line of body.split("\n")) {
      const m = /^\s*(--[a-z0-9-]+)\s*:\s*(.+);\s*$/.exec(line);
      if (m) declarations.set(m[1], m[2].trim());
    }
    if (declarations.size === 0) continue;
    for (const selector of selectorList.split(",")) {
      rules.push({ selector: selector.trim(), declarations, order });
    }
  }
  return rules;
}

const RULES = SHEETS.flatMap((css, order) => parseRules(css, order));

/** Specificity of a `:root[…][…]` selector, or null when the root does not match. */
function specificity(selector: string, attributes: Record<string, string>): number | null {
  if (!selector.startsWith(":root")) return null;
  const conditions = [...selector.matchAll(/\[([a-z-]+)(?:="([^"]*)")?\]/g)];
  for (const [, name, value] of conditions) {
    const present = attributes[name];
    if (present === undefined) return null;
    if (value !== undefined && present !== value) return null;
  }
  return 1 + conditions.length;
}

/** What the root carries with the given attributes set, cascade order applied. */
function rootWith(attributes: Record<string, string>): Map<string, string> {
  const winners = new Map<string, { value: string; rank: number }>();
  RULES.forEach((rule, index) => {
    const spec = specificity(rule.selector, attributes);
    if (spec === null) return;
    const rank = spec * 100000 + rule.order * 1000 + index;
    for (const [name, value] of rule.declarations) {
      const current = winners.get(name);
      if (!current || rank >= current.rank) winners.set(name, { value, rank });
    }
  });
  return new Map([...winners].map(([name, { value }]) => [name, value]));
}

function deref(value: string, declarations: Map<string, string>, depth = 0): string {
  if (depth > 16) throw new Error(`unresolved var() chain in ${value}`);
  const m = /var\((--[a-z0-9-]+)(?:,\s*([^)]*))?\)/.exec(value);
  if (!m) return value.replace(/\s+/g, " ").trim();
  const target = declarations.get(m[1]) ?? m[2] ?? "";
  const next = value.slice(0, m.index) + target + value.slice(m.index + m[0].length);
  return deref(next, declarations, depth + 1);
}

const ORIGIN_DECLARATIONS = new Map(Object.entries(ORIGIN_ROOT));
const NO_ATTRIBUTES = rootWith({});
const DARK = rootWith({ "data-theme": "dark" });

/**
 * Names the visual flip repaints, each for a stated reason:
 *
 * - `--writ-accent-hover`, `--writ-border-soft`, `--writ-font-mono`,
 *   `--writ-shadow-chip`, `--writ-shadow-modal`, `--writ-shadow-popover`,
 *   `--writ-sidebar-width`: the legacy layer froze the ADR-030 name at its
 *   pre-030 value. The flip lets the generated value through.
 * - `--writ-editor-font-size`: no longer a frozen alias. It is a token in the
 *   new vocabulary, resolving to the prose size, and editorZoom still writes
 *   the live value onto the root.
 * - `--writ-selection`: same, derived from `--writ-accent` rather than the
 *   retired `--writ-accent-default`.
 * - `--writ-line-height`: removed. Its one reader, global.css, takes the UI
 *   scale's own line height.
 * - `--writ-status-*` and `--writ-syntax-*`: light-first defaults, with the
 *   syntax set sourced from the neutrals (ADR-030 decision 3). A preset still
 *   overwrites both groups at runtime.
 * - `--writ-warning-foreground`: unchanged expression, new value, because it
 *   is mixed from `--writ-status-warning`.
 * - `--writ-traffic-minimize`: the lights mirror their host, and the baseline
 *   reads the system amber as #FEBC2E.
 */
const REPAINTED = [
  "--writ-accent-hover",
  "--writ-border-soft",
  "--writ-editor-font-size",
  "--writ-font-mono",
  "--writ-line-height",
  "--writ-selection",
  "--writ-shadow-chip",
  "--writ-shadow-modal",
  "--writ-shadow-popover",
  "--writ-sidebar-width",
  "--writ-status-error",
  "--writ-status-foreground",
  "--writ-status-success",
  "--writ-status-warning",
  "--writ-syntax-comment",
  "--writ-syntax-function",
  "--writ-syntax-keyword",
  "--writ-syntax-number",
  "--writ-syntax-string",
  "--writ-syntax-type",
  "--writ-syntax-variable",
  "--writ-traffic-minimize",
  "--writ-warning-foreground",
];

// Every other property the old sheet declared. The colour tiers are here too:
// the theme store overwrites them inline from the active preset, but with no
// script and no attribute the static fallback still has to paint 0.3.5.
const FROZEN = [
  "--writ-accent-default",
  "--writ-accent-foreground",
  "--writ-border-default",
  "--writ-border-focus",
  "--writ-border-pill",
  "--writ-font-sans",
  "--writ-font-size",
  "--writ-font-size-sm",
  "--writ-font-size-xs",
  "--writ-foreground-default",
  "--writ-foreground-muted",
  "--writ-foreground-subtle",
  "--writ-overlay-hover",
  "--writ-overlay-scrim",
  "--writ-overlay-subtle",
  "--writ-radius-1",
  "--writ-radius-2",
  "--writ-radius-3",
  "--writ-shadow-banner",
  "--writ-shadow-overlay",
  "--writ-shadow-xs",
  "--writ-space-1",
  "--writ-space-2",
  "--writ-space-3",
  "--writ-space-4",
  "--writ-space-5",
  "--writ-space-6",
  "--writ-statusbar-height",
  "--writ-surface-background",
  "--writ-surface-elevated",
  "--writ-surface-hover",
  "--writ-surface-input",
  "--writ-surface-raised",
  "--writ-surface-sunken",
  "--writ-traffic-blurred",
  "--writ-traffic-close",
  "--writ-traffic-close-glyph",
  "--writ-traffic-maximize",
  "--writ-traffic-maximize-glyph",
  "--writ-traffic-minimize-glyph",
];

/**
 * Names the legacy layer no longer carries, because the last stylesheet that
 * read one dropped it. The sidebar took its shadow off with the baseline pass:
 * the surface is one hairline now. The three tab names went with the borderless
 * strip, whose only reader was TabBar.css. The dialog and toast shadows went
 * with the menus and dialogs pass: both surfaces read the ADR-030 shadows. The
 * three window names went with the platform chrome pass: the caption row is
 * 32px on Windows and 47px on GNOME rather than one height, the close button
 * reads `--writ-win-close-*`, and the frame radius is `--writ-r-window`.
 */
const RETIRED = [
  "--writ-bg-tab-pill",
  "--writ-shadow-dialog",
  "--writ-shadow-sidebar",
  "--writ-shadow-toast",
  "--writ-tab-pill-height",
  "--writ-tabbar-height",
  "--writ-titlebar-height",
  "--writ-winctrl-danger-bg",
  "--writ-winctrl-danger-fg",
  "--writ-window-radius",
];

describe("token pipeline acceptance", () => {
  it("resolved token set is unchanged by the pipeline", () => {
    themeStore.resetOverrides();
    // Pinned, not followed: warp-dark and warp-light are a pair, so a system
    // polarity would resolve the light half on a light host.
    themeStore.setAppearance({ polarity: "dark", accent: "pine", prose_face: "system" });
    themeStore.setPreset("warp-dark");
    themeStore.applyToRoot(document.createElement("div"));
    const snapshot = JSON.parse(localStorage.getItem("writ-theme-vars-v2") as string) as {
      vars: Record<string, string>;
      attrs: Record<string, string>;
    };
    // The accent is its own axis now, so the three accent properties carry the
    // chosen hue rather than the preset's. Everything else is 0.3.5's warp-dark.
    const pine = ACCENTS.pine.dark;
    const expected = {
      ...RESOLVED_FIXTURE,
      "--writ-accent-default": pine.base,
      "--writ-accent-hover": pine.hover,
      "--writ-accent-foreground": pine.foreground,
    };
    expect(Object.keys(snapshot.vars).sort()).toEqual(Object.keys(expected).sort());
    expect(snapshot.vars).toEqual(expected);
    expect(snapshot.attrs).toEqual({ "data-theme": "dark", "data-accent": "pine" });
  });

  it("accounts for every property origin/main declared on :root", () => {
    expect([...FROZEN, ...REPAINTED, ...RETIRED].sort()).toEqual(Object.keys(ORIGIN_ROOT).sort());
    expect(FROZEN.filter((name) => REPAINTED.includes(name))).toEqual([]);
  });

  for (const [state, resolved] of [
    ["no attribute", NO_ATTRIBUTES],
    ['data-theme="dark"', DARK],
  ] as const) {
    it(`every frozen property resolves to its origin/main value with ${state}`, () => {
      const drifted: string[] = [];
      for (const name of FROZEN) {
        const declaration = resolved.get(name);
        if (declaration === undefined) {
          drifted.push(`${name} is not declared`);
          continue;
        }
        const before = deref(ORIGIN_ROOT[name], ORIGIN_DECLARATIONS);
        const after = deref(declaration, resolved);
        if (before !== after) drifted.push(`${name}\n  was: ${before}\n  now: ${after}`);
      }
      expect(drifted, drifted.join("\n")).toEqual([]);
    });
  }

  it("the legacy layer declares nothing the generated sheet also declares", () => {
    // Keeps its selector list at the specificity of :root[data-theme="dark"],
    // so a name it does re-declare would still win on source order — and none
    // does any more.
    expect(LEGACY_CSS).toContain(":root,\n:root[data-theme] {");
    const legacyNames = new Set(
      [...LEGACY_CSS.matchAll(/^\s*(--[a-z0-9-]+)\s*:/gm)].map((m) => m[1]),
    );
    const themeNames = new Set(
      [...SHEETS[0].matchAll(/^\s*(--[a-z0-9-]+)\s*:/gm)].map((m) => m[1]),
    );
    expect([...legacyNames].filter((name) => themeNames.has(name))).toEqual([]);
  });

  it("a retired name is declared nowhere and read nowhere", () => {
    for (const name of RETIRED) {
      expect(NO_ATTRIBUTES.has(name), name).toBe(false);
      expect(DARK.has(name), name).toBe(false);
    }
  });

  it("every repainted name still resolves to something", () => {
    // A repaint is a new value, never a dangling var(): the one name this unit
    // removes outright is --writ-line-height, whose reader went with it.
    for (const name of REPAINTED) {
      if (name === "--writ-line-height") {
        expect(NO_ATTRIBUTES.has(name), name).toBe(false);
        continue;
      }
      expect(NO_ATTRIBUTES.get(name), name).toBeDefined();
      expect(deref(NO_ATTRIBUTES.get(name)!, NO_ATTRIBUTES)).not.toBe(
        deref(ORIGIN_ROOT[name], ORIGIN_DECLARATIONS),
      );
    }
  });
});
