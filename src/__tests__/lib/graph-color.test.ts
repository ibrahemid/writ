import { describe, it, expect } from "vitest";
import {
  folderColors,
  formatColor,
  parseColor,
  rotateHue,
  toOklch,
  toRgb,
} from "../../lib/graph/color";

// A folder's colour is the theme's accent turned round the hue circle. What
// these hold is that the turn is reversible, that a folder's colour depends on
// its own place in the sorted list and nothing else, and that a theme whose
// accent cannot be read paints no colours rather than a guess.

const ACCENT_LIGHT = "#1F6F5C";
const ACCENT_DARK = "#5BC5A7";

describe("reading a colour out of the stylesheet", () => {
  it("reads the hex a theme writes", () => {
    expect(parseColor(ACCENT_LIGHT)).toEqual({ r: 0x1f, g: 0x6f, b: 0x5c });
  });

  it("reads a short hex as the doubled digits", () => {
    expect(parseColor("#1a4")).toEqual({ r: 0x11, g: 0xaa, b: 0x44 });
  });

  it("reads what a browser hands back for a resolved colour", () => {
    expect(parseColor("rgb(31, 111, 92)")).toEqual({ r: 31, g: 111, b: 92 });
    expect(parseColor("rgba(31 111 92 / 0.5)")).toEqual({ r: 31, g: 111, b: 92 });
  });

  it("reads nothing out of a colour it does not know", () => {
    expect(parseColor("color-mix(in srgb, red, blue)")).toBeNull();
    expect(parseColor("")).toBeNull();
  });
});

describe("the accent in OKLCH", () => {
  it("comes back the colour it went in as", () => {
    for (const accent of [ACCENT_LIGHT, ACCENT_DARK, "#FFFFFF", "#000000", "#7C3AED"]) {
      const rgb = parseColor(accent);
      expect(rgb).not.toBeNull();
      const back = toRgb(toOklch(rgb as { r: number; g: number; b: number }));
      expect(Math.abs(back.r - (rgb?.r ?? 0))).toBeLessThanOrEqual(1);
      expect(Math.abs(back.g - (rgb?.g ?? 0))).toBeLessThanOrEqual(1);
      expect(Math.abs(back.b - (rgb?.b ?? 0))).toBeLessThanOrEqual(1);
    }
  });

  it("keeps its lightness and its chroma when it is turned", () => {
    const base = toOklch(parseColor(ACCENT_LIGHT) as { r: number; g: number; b: number });
    const turned = rotateHue(base, 137.508);
    expect(turned.l).toBeCloseTo(base.l, 12);
    expect(turned.c).toBeCloseTo(base.c, 12);
    expect(turned.h).not.toBeCloseTo(base.h, 3);
  });

  it("comes back round the circle after a whole turn", () => {
    const base = toOklch(parseColor(ACCENT_DARK) as { r: number; g: number; b: number });
    expect(rotateHue(base, 360).h).toBeCloseTo(base.h, 9);
    expect(rotateHue(base, -400).h).toBeCloseTo(rotateHue(base, -40).h, 9);
  });

  it("is written as something a canvas can be told to paint with", () => {
    expect(formatColor({ r: 31.4, g: 111.6, b: 92 })).toBe("#1f705c");
  });
});

describe("a colour per folder", () => {
  const FOLDERS = ["Projects", "Archive", "Daily"];

  it("gives one folder one colour and two folders two", () => {
    const colors = folderColors(ACCENT_LIGHT, FOLDERS);
    expect(colors.size).toBe(3);
    expect(new Set(colors.values()).size).toBe(3);
    expect(folderColors(ACCENT_LIGHT, ["Projects", "Projects"]).size).toBe(1);
  });

  it("gives a folder the same colour whichever order the notes came back in", () => {
    const one = folderColors(ACCENT_LIGHT, FOLDERS);
    const other = folderColors(ACCENT_LIGHT, [...FOLDERS].reverse());
    for (const folder of FOLDERS) expect(other.get(folder)).toBe(one.get(folder));
  });

  it("gives the first folder the accent itself", () => {
    const colors = folderColors(ACCENT_LIGHT, ["Archive"]);
    const accent = parseColor(ACCENT_LIGHT) as { r: number; g: number; b: number };
    expect(colors.get("Archive")).toBe(formatColor(toRgb(toOklch(accent))));
  });

  it("takes a folder's colour from its own place in the list", () => {
    // "Zed" sorts last either way, so what it is painted with does not move
    // when a folder is added ahead of it in the list but behind it in sort.
    const before = folderColors(ACCENT_LIGHT, ["Archive", "Zed"]);
    const after = folderColors(ACCENT_LIGHT, ["Zed", "Archive"]);
    expect(after.get("Zed")).toBe(before.get("Zed"));
  });

  it("gives a note in the root of the notes folder no colour of its own", () => {
    const colors = folderColors(ACCENT_LIGHT, ["", "Archive"]);
    expect(colors.has("")).toBe(false);
    expect(colors.size).toBe(1);
  });

  it("gives no colours at all when the accent cannot be read", () => {
    expect(folderColors("var(--something-else)", FOLDERS).size).toBe(0);
  });

  it("follows the theme: a different accent is a different set of colours", () => {
    const light = folderColors(ACCENT_LIGHT, FOLDERS);
    const dark = folderColors(ACCENT_DARK, FOLDERS);
    for (const folder of FOLDERS) expect(dark.get(folder)).not.toBe(light.get(folder));
  });
});
