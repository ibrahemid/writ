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

// Every property the old sheet declared. The colour tiers are here too: the
// theme store overwrites them inline from the active preset, but with no script
// and no attribute the static fallback still has to paint 0.3.5.
const FROZEN = [
  "--writ-accent-default",
  "--writ-accent-foreground",
  "--writ-accent-hover",
  "--writ-bg-tab-pill",
  "--writ-border-default",
  "--writ-border-focus",
  "--writ-border-pill",
  "--writ-border-soft",
  "--writ-editor-font-size",
  "--writ-font-mono",
  "--writ-font-sans",
  "--writ-font-size",
  "--writ-font-size-sm",
  "--writ-font-size-xs",
  "--writ-foreground-default",
  "--writ-foreground-muted",
  "--writ-foreground-subtle",
  "--writ-line-height",
  "--writ-overlay-hover",
  "--writ-overlay-scrim",
  "--writ-overlay-subtle",
  "--writ-radius-1",
  "--writ-radius-2",
  "--writ-radius-3",
  "--writ-selection",
  "--writ-shadow-banner",
  "--writ-shadow-chip",
  "--writ-shadow-dialog",
  "--writ-shadow-modal",
  "--writ-shadow-overlay",
  "--writ-shadow-popover",
  "--writ-shadow-sidebar",
  "--writ-shadow-toast",
  "--writ-shadow-xs",
  "--writ-sidebar-width",
  "--writ-space-1",
  "--writ-space-2",
  "--writ-space-3",
  "--writ-space-4",
  "--writ-space-5",
  "--writ-space-6",
  "--writ-status-error",
  "--writ-status-foreground",
  "--writ-status-success",
  "--writ-status-warning",
  "--writ-statusbar-height",
  "--writ-surface-background",
  "--writ-surface-elevated",
  "--writ-surface-hover",
  "--writ-surface-input",
  "--writ-surface-raised",
  "--writ-surface-sunken",
  "--writ-syntax-comment",
  "--writ-syntax-function",
  "--writ-syntax-keyword",
  "--writ-syntax-number",
  "--writ-syntax-string",
  "--writ-syntax-type",
  "--writ-syntax-variable",
  "--writ-tab-pill-height",
  "--writ-tabbar-height",
  "--writ-titlebar-height",
  "--writ-traffic-blurred",
  "--writ-traffic-close",
  "--writ-traffic-close-glyph",
  "--writ-traffic-maximize",
  "--writ-traffic-maximize-glyph",
  "--writ-traffic-minimize",
  "--writ-traffic-minimize-glyph",
  "--writ-warning-foreground",
  "--writ-winctrl-danger-bg",
  "--writ-winctrl-danger-fg",
  "--writ-window-radius",];

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

  it("covers every property origin/main declared on :root", () => {
    expect([...FROZEN].sort()).toEqual(Object.keys(ORIGIN_ROOT).sort());
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

  it("the re-declared names outrank the dark and platform layers", () => {
    // Same specificity as :root[data-theme="dark"] and :root[data-platform="win"],
    // and the legacy sheet is imported after the generated one, so source order
    // settles it.
    expect(LEGACY_CSS).toContain(":root,\n:root[data-theme] {");
    for (const name of [
      "--writ-shadow-modal",
      "--writ-shadow-popover",
      "--writ-shadow-chip",
      "--writ-font-mono",
    ]) {
      expect(NO_ATTRIBUTES.get(name), name).toBe(DARK.get(name));
    }
  });
});
