import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

// The chrome rules the three areas each pinned over their own file list, stated
// once over the repo: one focus ring, one stacking scale, one interface text
// size, one scrollbar. An area that lands later inherits them by existing.
// The chat pane is out of scope here; it carries its own sheet and its own pass.

const REPO_ROOT = process.cwd();

export function stylesheets(dir: string, found: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry !== "node_modules" && entry !== "Chat") stylesheets(full, found);
    } else if (entry.endsWith(".css")) {
      found.push(full);
    }
  }
  return found;
}

const ALL = stylesheets(resolve(REPO_ROOT, "src")).map(
  (file) => [relative(REPO_ROOT, file), readFileSync(file, "utf8")] as const,
);
const COMPONENTS = ALL.filter(([rel]) => rel.startsWith("src/components/"));

/** A ring built from the accent by hand instead of the focus properties. */
export const ACCENT_RING = /(?<![\w-])outline:\s*\d+px solid var\(--writ-accent\)/;

/** A stacking order written as a number rather than spent from the scale. */
export const BARE_Z_INDEX = /(?<![\w-])z-index:\s*-?\d/;

/** A text size that the interface text setting cannot move. */
export const PX_FONT_SIZE = /font-size:\s*[\d.]+px/;

/** The ring cancelled in CSS, where no element says why. */
export const OUTLINE_NONE = /outline:\s*none/;

// Stacking inside one overlay, not a layer of the chrome: the scale starts at
// --writ-z-chrome (100) and has no name for "above the drawing it sits on".
// Whether these become a token or stay numbers is still open.
export const BARE_Z_INDEX_ALLOWLIST: readonly string[] = [
  "src/components/Graph/FolderGraphView.css",
];

// focus.css makes data-writ-focus-silent the one opt-out, so a rule that
// cancels the ring in CSS cancels it for every shell at once. A selector may
// sit here only with the reason it cannot carry the attribute.
export const OUTLINE_NONE_ALLOWLIST: readonly { file: string; selector: string; reason: string }[] =
  [];

function offendersOf(
  sheets: readonly (readonly [string, string])[],
  pattern: RegExp,
  skip: readonly string[] = [],
): string[] {
  return sheets
    .filter(([rel, css]) => !skip.includes(rel) && pattern.test(css))
    .map(([rel]) => rel);
}

/** Selector lists that name a scrollbar part, with the rule body beside them. */
export function scrollbarRules(css: string): { selector: string; body: string }[] {
  return [...css.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/([^{}]+)\{([^}]*)\}/g)]
    .filter(([, list]) => list.includes("::-webkit-scrollbar"))
    .map(([, list, body]) => ({ selector: list.trim(), body }));
}

/**
 * A scrollbar rule belongs to the platform layer. The one thing a component may
 * do is hide the bar on a strip it scrolls itself, which is a zero size on the
 * bar and nothing on the rail or the thumb.
 */
export function scrollbarOffenders(css: string): string[] {
  return scrollbarRules(css)
    .filter(({ selector, body }) => {
      const scoped =
        selector.startsWith(':root[data-platform="win"]') ||
        selector.startsWith(':root[data-platform="linux"]');
      const hides = !/-(?:thumb|track)\b/.test(selector) && /:\s*0\s*;/.test(body);
      return !scoped && !hides;
    })
    .map(({ selector }) => selector);
}

describe("the focus ring across the interface", () => {
  it("is never hand-written from the accent", () => {
    const offenders = offendersOf(ALL, ACCENT_RING);
    expect(offenders, `hand-written focus rings: ${offenders.join(", ")}`).toEqual([]);
  });

  it("is cancelled through the attribute, not in a component sheet", () => {
    const allowed = OUTLINE_NONE_ALLOWLIST.map(({ file }) => file);
    const offenders = offendersOf(COMPONENTS, OUTLINE_NONE, allowed);
    expect(offenders, `outline: none in CSS: ${offenders.join(", ")}`).toEqual([]);
  });

  it("is silenced on the element wherever a control is deliberately quiet", () => {
    const focus = readFileSync(resolve(REPO_ROOT, "src/styles/focus.css"), "utf8");
    expect(focus).toMatch(/\[data-writ-focus-silent\]/);
  });
});

describe("the stacking scale", () => {
  it("is spent by name in every component sheet", () => {
    const offenders = offendersOf(COMPONENTS, BARE_Z_INDEX, BARE_Z_INDEX_ALLOWLIST);
    expect(offenders, `raw stacking numbers: ${offenders.join(", ")}`).toEqual([]);
  });
});

describe("the interface text size", () => {
  it("moves every label in every component sheet", () => {
    const offenders = offendersOf(COMPONENTS, PX_FONT_SIZE);
    expect(offenders, `literal font sizes: ${offenders.join(", ")}`).toEqual([]);
  });
});

describe("the scrollbars", () => {
  it("are drawn by the platform layer alone", () => {
    const offenders: string[] = [];
    for (const [rel, css] of ALL) {
      for (const selector of scrollbarOffenders(css)) offenders.push(`${rel} -> ${selector}`);
    }
    expect(offenders, offenders.join("\n")).toEqual([]);
  });

  it("are stated somewhere, so the rule above is not vacuous", () => {
    const rules = ALL.flatMap(([, css]) => scrollbarRules(css));
    expect(rules.length).toBeGreaterThan(0);
  });
});
