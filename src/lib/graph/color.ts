/**
 * The one colour the theme gives the graph, turned into a colour per folder.
 *
 * A folder's notes are drawn in the accent the theme is already spending on
 * links and the caret, rotated round the hue circle by which folder it is.
 * Rotating in OKLCH rather than in sRGB is what keeps every folder's colour
 * the same weight: the same lightness and the same chroma as the accent, so
 * no folder shouts and none disappears against the ground. Nothing here is a
 * colour: the accent comes out of the stylesheet at runtime, and a theme that
 * changes it changes every folder with it.
 */

/** A colour as the stylesheet wrote it, 0–255 per channel. */
export interface Rgb {
  r: number;
  g: number;
  b: number;
}

/** The same colour as a lightness, a chroma and a hue in degrees. */
export interface Oklch {
  l: number;
  c: number;
  h: number;
}

/**
 * How far apart two folders' hues are.
 *
 * The golden angle, so the first folders land far apart and the twentieth
 * still lands in a gap rather than on top of the first. Sharing the circle out
 * by how many folders there are would repaint every folder the moment one is
 * added; this way a folder's colour depends on its own place in the list only.
 */
const HUE_STEP = 137.508;

const HEX_PATTERN = /^#([0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/i;
const RGB_PATTERN = /^rgba?\(([^)]+)\)$/i;

function clampChannel(value: number): number {
  if (!(value > 0)) return 0;
  return value > 255 ? 255 : value;
}

/**
 * A CSS colour as channels, or `null` for anything else.
 *
 * Hex and `rgb()` are what a custom property holds once the browser has
 * resolved it. A theme that writes something else is not guessed at: the
 * caller falls back to a token it can paint with instead.
 */
export function parseColor(css: string): Rgb | null {
  const text = css.trim();

  const hex = HEX_PATTERN.exec(text);
  if (hex) {
    const digits = hex[1];
    const short = digits.length <= 4;
    const at = (index: number) =>
      short
        ? Number.parseInt(digits[index] + digits[index], 16)
        : Number.parseInt(digits.slice(index * 2, index * 2 + 2), 16);
    return { r: at(0), g: at(1), b: at(2) };
  }

  const rgb = RGB_PATTERN.exec(text);
  if (rgb) {
    const parts = rgb[1]
      .split(/[\s,/]+/)
      .filter((part) => part.length > 0)
      .map((part) => (part.endsWith("%") ? (Number.parseFloat(part) * 255) / 100 : Number(part)));
    if (parts.length < 3 || parts.slice(0, 3).some((part) => Number.isNaN(part))) return null;
    return { r: clampChannel(parts[0]), g: clampChannel(parts[1]), b: clampChannel(parts[2]) };
  }

  return null;
}

/** A channel out of its gamma curve, 0–1. */
function toLinear(channel: number): number {
  const value = channel / 255;
  return value <= 0.04045 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4);
}

/** A linear channel back under the gamma curve, 0–255. */
function fromLinear(value: number): number {
  const curved = value <= 0.0031308 ? value * 12.92 : 1.055 * Math.pow(value, 1 / 2.4) - 0.055;
  return clampChannel(Math.round(curved * 255));
}

/** Björn Ottosson's OKLab, and its polar form. */
export function toOklch(color: Rgb): Oklch {
  const r = toLinear(color.r);
  const g = toLinear(color.g);
  const b = toLinear(color.b);

  const long = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const medium = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const short = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);

  const l = 0.2104542553 * long + 0.793617785 * medium - 0.0040720468 * short;
  const a = 1.9779984951 * long - 2.428592205 * medium + 0.4505937099 * short;
  const bb = 0.0259040371 * long + 0.7827717662 * medium - 0.808675766 * short;

  const hue = (Math.atan2(bb, a) * 180) / Math.PI;
  return { l, c: Math.sqrt(a * a + bb * bb), h: hue < 0 ? hue + 360 : hue };
}

/** The same colour back in channels, clipped to what a screen can show. */
export function toRgb(color: Oklch): Rgb {
  const radians = (color.h * Math.PI) / 180;
  const a = color.c * Math.cos(radians);
  const b = color.c * Math.sin(radians);

  const long = (color.l + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const medium = (color.l - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const short = (color.l - 0.0894841775 * a - 1.291485548 * b) ** 3;

  return {
    r: fromLinear(4.0767416621 * long - 3.3077115913 * medium + 0.2309699292 * short),
    g: fromLinear(-1.2684380046 * long + 2.6097574011 * medium - 0.3413193965 * short),
    b: fromLinear(-0.0041960863 * long - 0.7034186147 * medium + 1.707614701 * short),
  };
}

/** A colour the canvas can be told to paint with, as the theme writes one. */
export function formatColor(color: Rgb): string {
  const digits = (channel: number) => Math.round(channel).toString(16).padStart(2, "0");
  return `#${digits(color.r)}${digits(color.g)}${digits(color.b)}`;
}

/** The same colour, turned round the hue circle. */
export function rotateHue(color: Oklch, degrees: number): Oklch {
  const turned = (color.h + degrees) % 360;
  return { ...color, h: turned < 0 ? turned + 360 : turned };
}

/**
 * A colour per top-level folder, in the order the folders sort in.
 *
 * The list is sorted here rather than trusted from the caller, so the same
 * folder keeps the same colour whichever order the notes came back in. A note
 * in the root of the notes folder is in no folder and takes no colour: the
 * caller paints it with the muted foreground instead.
 *
 * An accent the parser cannot read hands back an empty map, and every note is
 * drawn in the token the canvas already has.
 */
export function folderColors(accent: string, folders: Iterable<string>): Map<string, string> {
  const colors = new Map<string, string>();
  const parsed = parseColor(accent);
  if (!parsed) return colors;

  const base = toOklch(parsed);
  const sorted = [...new Set(folders)].filter((folder) => folder.length > 0).sort();
  sorted.forEach((folder, index) => {
    colors.set(folder, formatColor(toRgb(rotateHue(base, index * HUE_STEP))));
  });
  return colors;
}
