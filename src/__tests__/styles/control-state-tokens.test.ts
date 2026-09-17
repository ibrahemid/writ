import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const ROOT = process.cwd();

function sheet(rel: string): string {
  return readFileSync(resolve(ROOT, rel), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
}

function declarations(css: string, selector: string): Map<string, string> {
  for (const [, list, body] of css.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    if (!list.split(",").some((s) => s.trim() === selector)) continue;
    const found = new Map<string, string>();
    for (const line of body.split(";")) {
      const [property, ...rest] = line.split(":");
      if (rest.length === 0) continue;
      found.set(property.trim(), rest.join(":").trim());
    }
    return found;
  }
  throw new Error(`no rule for ${selector}`);
}

const BUTTON = sheet("src/components/Button/Button.css");
const THEME = sheet("src/styles/generated/theme.css");

describe("hover and disabled states resolve through tokens", () => {
  it("the danger primary swaps a colour on hover, as every other button does", () => {
    const hover = declarations(BUTTON, ".writ-btn-primary.writ-btn-danger:hover:not(:disabled)");
    expect(hover.get("background")).toBe("var(--writ-danger-hover)");
    expect(BUTTON).not.toContain("filter:");
  });

  it("the danger hover is a deeper red in both schemes", () => {
    const light = declarations(THEME, ":root");
    const dark = declarations(THEME, ':root[data-theme="dark"]');
    for (const [scheme, decls] of [
      ["light", light],
      ["dark", dark],
    ] as const) {
      const base = decls.get("--writ-danger");
      const hover = decls.get("--writ-danger-hover");
      expect(hover, `${scheme} danger hover`).toBeDefined();
      expect(hover).not.toBe(base);
      expect(luminance(hover!), `${scheme} hover is deeper than the fill`).toBeLessThan(
        luminance(base!),
      );
    }
  });

  it("a disabled control dims by the one token", () => {
    // Button and the settings select are retokenised here. The other five
    // carriers (.settings-accent, .settings-switch, the two find buttons and
    // the spelling apply) belong to areas with their own passes and still
    // carry a literal; they join this rule when those land.
    expect(declarations(THEME, ":root").get("--writ-disabled-opacity")).toBe("0.5");
    for (const [rel, selector] of [
      ["src/components/Button/Button.css", ".writ-btn:disabled"],
      ["src/components/SettingsModal/SettingsModal.css", ".settings-select:disabled"],
    ] as const) {
      expect(declarations(sheet(rel), selector).get("opacity"), selector).toBe(
        "var(--writ-disabled-opacity)",
      );
    }
    // `opacity: 1` on the button's icon is a reset to full, not a dim.
    expect(/opacity\s*:\s*0?\.\d+/.test(BUTTON), "Button.css still dims by a literal").toBe(false);
  });
});

/** Relative luminance of a #rrggbb value, enough to order two tints apart. */
function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((i) => Number.parseInt(hex.slice(i, i + 2), 16) / 255);
  const lin = (c: number) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}
