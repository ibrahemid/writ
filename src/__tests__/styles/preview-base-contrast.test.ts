import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// ADR-009 §"Accessibility": the fallback stylesheet ships with computed
// contrast ratios; this regression test asserts WCAG AA (4.5:1 body,
// 3.0:1 large text) for every theme combination the stylesheet defines.

const CSS_PATH = resolve(process.cwd(), "src-tauri/assets/preview-base.css");
const css = readFileSync(CSS_PATH, "utf8");

function blockFor(selector: string): string {
  const esc = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const re = new RegExp(`${esc}\\s*\\{([\\s\\S]*?)\\}`, "m");
  const m = css.match(re);
  if (!m) throw new Error(`selector block not found: ${selector}`);
  return m[1];
}

function token(block: string, name: string): string {
  const re = new RegExp(`--writ-preview-${name}\\s*:\\s*([^;]+);`);
  const m = block.match(re);
  if (!m) throw new Error(`token --writ-preview-${name} not found`);
  return m[1].trim();
}

function srgbToLinear(channel: number): number {
  const s = channel / 255;
  return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
}

function luminance(hex: string): number {
  const m = /^#([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) throw new Error(`not a 6-digit hex color: ${hex}`);
  const n = parseInt(m[1], 16);
  const r = (n >> 16) & 0xff;
  const g = (n >> 8) & 0xff;
  const b = n & 0xff;
  return 0.2126 * srgbToLinear(r) + 0.7152 * srgbToLinear(g) + 0.0722 * srgbToLinear(b);
}

function contrast(a: string, b: string): number {
  const la = luminance(a);
  const lb = luminance(b);
  const [hi, lo] = la >= lb ? [la, lb] : [lb, la];
  return (hi + 0.05) / (lo + 0.05);
}

const PALETTES: { name: string; selector: string }[] = [
  { name: "dark", selector: ":root" },
  { name: "light", selector: '[data-writ-theme="light"]' },
];

describe("preview-base.css contrast (WCAG AA)", () => {
  for (const { name, selector } of PALETTES) {
    describe(`${name} palette`, () => {
      const block = blockFor(selector);
      const bg = token(block, "bg");
      const codeBg = token(block, "code-bg");

      it("body text meets 4.5:1 on the background", () => {
        expect(contrast(token(block, "fg"), bg)).toBeGreaterThanOrEqual(4.5);
      });

      it("muted text meets 4.5:1 on the background", () => {
        expect(contrast(token(block, "muted"), bg)).toBeGreaterThanOrEqual(4.5);
      });

      it("accent text meets 4.5:1 on the background", () => {
        expect(contrast(token(block, "accent"), bg)).toBeGreaterThanOrEqual(4.5);
      });

      it("code foreground meets 4.5:1 on the code background", () => {
        expect(contrast(token(block, "code-fg"), codeBg)).toBeGreaterThanOrEqual(4.5);
      });

      it("subtle text (large/decorative) meets at least 3.0:1 on the background", () => {
        expect(contrast(token(block, "subtle"), bg)).toBeGreaterThanOrEqual(3.0);
      });
    });
  }

  it("defines both a dark (:root) and a light ([data-writ-theme=light]) palette", () => {
    expect(css).toMatch(/:root\s*\{/);
    expect(css).toMatch(/\[data-writ-theme="light"\]\s*\{/);
  });
});
