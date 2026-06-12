import { describe, it, expect } from "vitest";
import { PRESETS } from "../../styles/themes";
import type { Theme } from "../../types/theme";

// Operator decision (2026-06-11): WCAG AA is enforced for EVERY shipped preset,
// not just the new light theme. Text tiers must clear 4.5:1; the decorative
// "subtle" tier and accent-as-UI clear the 3.0:1 large-text/non-text bar. On-fill
// text (button labels over an accent or status fill) clears 4.5:1.
//
// This is the regression oracle for the token re-tune. A preset that dips below
// the bar fails here before it can ship.

const AA_TEXT = 4.5;
const AA_LARGE = 3.0;

function srgbToLinear(channel: number): number {
  const s = channel / 255;
  return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
}

function luminance(hex: string): number {
  const m = /^#([0-9a-fA-F]{6})$/.exec(hex.trim());
  if (!m) throw new Error(`not a 6-digit hex color: ${hex}`);
  const n = parseInt(m[1], 16);
  return (
    0.2126 * srgbToLinear((n >> 16) & 0xff) +
    0.7152 * srgbToLinear((n >> 8) & 0xff) +
    0.0722 * srgbToLinear(n & 0xff)
  );
}

function contrast(a: string, b: string): number {
  const la = luminance(a);
  const lb = luminance(b);
  const [hi, lo] = la >= lb ? [la, lb] : [lb, la];
  return (hi + 0.05) / (lo + 0.05);
}

// Every surface a foreground tier can render on.
const SURFACE_KEYS = ["background", "sunken", "raised", "elevated", "input", "hover"] as const;

describe("app theme tokens contrast (WCAG AA, all presets)", () => {
  for (const preset of PRESETS as Theme[]) {
    describe(preset.name, () => {
      for (const surf of SURFACE_KEYS) {
        const bg = preset.surface[surf];

        it(`default text meets 4.5:1 on surface.${surf}`, () => {
          expect(contrast(preset.foreground.default, bg)).toBeGreaterThanOrEqual(AA_TEXT);
        });

        it(`muted text meets 4.5:1 on surface.${surf}`, () => {
          expect(contrast(preset.foreground.muted, bg)).toBeGreaterThanOrEqual(AA_TEXT);
        });

        it(`subtle text meets 3.0:1 on surface.${surf}`, () => {
          expect(contrast(preset.foreground.subtle, bg)).toBeGreaterThanOrEqual(AA_LARGE);
        });
      }

      it("accent reads as a UI element on the background (3.0:1)", () => {
        expect(contrast(preset.accent.default, preset.surface.background)).toBeGreaterThanOrEqual(AA_LARGE);
      });

      it("accent-foreground text meets 4.5:1 on the accent fill", () => {
        expect(contrast(preset.accent.foreground, preset.accent.default)).toBeGreaterThanOrEqual(AA_TEXT);
      });

      it("accent-foreground text meets 4.5:1 on the accent hover fill", () => {
        expect(contrast(preset.accent.foreground, preset.accent.hover)).toBeGreaterThanOrEqual(AA_TEXT);
      });

      it("status-foreground text meets 4.5:1 on the error fill", () => {
        expect(contrast(preset.status.foreground, preset.status.error)).toBeGreaterThanOrEqual(AA_TEXT);
      });
    });
  }

  it("covers every shipped preset", () => {
    expect(PRESETS.length).toBeGreaterThanOrEqual(5);
  });
});
