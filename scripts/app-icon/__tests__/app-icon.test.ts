// @vitest-environment node
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { IconBuildArgsError, isRepositoryRoot, parseBuildArgs, runAppIconBuild } from "../cli.mjs";
import {
  ICNS_LAYOUT,
  ICO_SIZES,
  IconFormatError,
  decodeIcnsImage,
  decodeIcnsRle,
  decodePng,
  encodeIcns,
  encodeIcnsRle,
  encodeIco,
  encodePng,
  parseIcns,
  parseIco,
} from "../containers.mjs";
import { APP_ICON_FILES, buildAppIconFiles, writeAppIconFiles } from "../outputs.mjs";
import {
  ICON_FORMS,
  IconParamsError,
  IconRenderError,
  PARAMS_PATH,
  ROOT,
  VARIANTS_DIR,
  buildIconSvg,
  computeBodyPx,
  computeStrokePx,
  layoutIcon,
  listIconVariants,
  loadIconParams,
  parseIconConfig,
  parseIconParams,
  parseIconVariant,
  renderIcon,
} from "../render.mjs";
import type { IconForm, IconLayout, IconParams, IconPoint, PrimitiveLayout, RasterImage } from "../render.mjs";

const RAW_CONFIG = JSON.parse(readFileSync(PARAMS_PATH, "utf8"));
const params = loadIconParams();
const variantNames = listIconVariants();
const otherVariants = variantNames.filter((name) => name !== params.shippedVariant);

const SHIPPED_SIZES: Record<IconForm, number[]> = {
  macos: [...new Set(ICNS_LAYOUT.map(({ size }) => size))].sort((a, b) => a - b),
  flat: [
    ...new Set([
      ...ICO_SIZES,
      ...APP_ICON_FILES.flatMap(({ kind, form, size }) => (kind === "png" && form === "flat" && size !== undefined ? [size] : [])),
    ]),
  ].sort((a, b) => a - b),
};

type RawRecord = Record<string, unknown>;

interface RawVariant {
  stroke: RawRecord;
  primitives: RawRecord[];
  optical: RawRecord[];
}

const SYNTHETIC_VARIANT: RawVariant = {
  stroke: { width: 40, widthForm: "macos", smallSizes: [[16, 1.5], [32, 2], [64, 3]], proportionalFrom: 128 },
  primitives: [
    { id: "stem", type: "polyline", points: [[512, 400], [512, 600]] },
    { id: "hook", type: "arc", center: [432, 600], radius: 80, startDegrees: 0, sweepDegrees: 180 },
    { id: "dot", type: "dot", center: [512, 300], diameterInStrokes: 2 },
  ],
  optical: [
    { target: "glyph", forms: ["macos"], sizes: [16], translatePx: [0.5, -0.25] },
    { target: "dot", forms: ["macos", "flat"], sizes: [16], translatePx: [0, -1], diameterPx: 3 },
  ],
};

function readVariantRaw(name: string) {
  return JSON.parse(readFileSync(join(VARIANTS_DIR, `${name}.json`), "utf8"));
}

function mutate<T>(raw: T, change: (copy: T) => void): T {
  const copy = structuredClone(raw);
  change(copy);
  return copy;
}

function parseSynthetic(change: (raw: RawVariant) => void = () => {}): IconParams {
  return parseIconParams(RAW_CONFIG, mutate(SYNTHETIC_VARIANT, change), "synthetic");
}

function createPatternImage(size: number): RasterImage {
  const rgba = new Uint8Array(size * size * 4);
  for (let index = 0; index < size * size; index++) {
    rgba[index * 4] = (Math.floor(index / 7) % 3) * 90;
    rgba[index * 4 + 1] = (index * 37) % 256;
    rgba[index * 4 + 2] = index % 5 === 0 ? 255 : 12;
    rgba[index * 4 + 3] = (index * 11) % 256;
  }
  return { width: size, height: size, rgba };
}

function findPixelMismatch(expected: RasterImage, actual: RasterImage): string | null {
  if (expected.width !== actual.width || expected.height !== actual.height) {
    return `size ${actual.width}x${actual.height}, expected ${expected.width}x${expected.height}`;
  }
  for (let index = 0; index < expected.rgba.length; index++) {
    if (expected.rgba[index] !== actual.rgba[index]) {
      const pixel = index >> 2;
      return `pixel (${pixel % expected.width}, ${Math.floor(pixel / expected.width)}) channel ${index % 4} is ${actual.rgba[index]}, expected ${expected.rgba[index]}`;
    }
  }
  return null;
}

function readPixel(image: RasterImage, x: number, y: number): number[] {
  const offset = (y * image.width + x) * 4;
  return Array.from(image.rgba.subarray(offset, offset + 4));
}

function findPrimitive(layout: IconLayout, id: string): PrimitiveLayout {
  const primitive = layout.primitives.find((candidate) => candidate.id === id);
  if (primitive === undefined) throw new Error(`layout has no primitive ${id}`);
  return primitive;
}

function readAnchor(primitive: PrimitiveLayout): IconPoint {
  return primitive.type === "polyline" ? primitive.points[0] : primitive.center;
}

function readCentrelinePoint(primitive: PrimitiveLayout): IconPoint {
  switch (primitive.type) {
    case "polyline": {
      const [[ax, ay], [bx, by]] = primitive.points;
      return [(ax + bx) / 2, (ay + by) / 2];
    }
    case "arc": {
      const radians = ((primitive.startDegrees + primitive.sweepDegrees / 2) * Math.PI) / 180;
      return [primitive.center[0] + primitive.radius * Math.cos(radians), primitive.center[1] + primitive.radius * Math.sin(radians)];
    }
    default:
      return primitive.center;
  }
}

function expectShift(actual: IconPoint, base: IconPoint, [dx, dy]: IconPoint) {
  expect(actual[0]).toBeCloseTo(base[0] + dx, 10);
  expect(actual[1]).toBeCloseTo(base[1] + dy, 10);
}

function readTrackedFiles(): Map<string, Buffer> {
  return new Map(APP_ICON_FILES.map(({ path }) => [path, readFileSync(join(ROOT, path))]));
}

describe("app icon config and variants", () => {
  it("names a shipped variant that exists among the variant files", () => {
    expect(variantNames).toEqual(["j-angle", "j-cap", "j-dot", "j-dot-bold"]);
    expect(variantNames).toContain(RAW_CONFIG.variant);
    expect(params.shippedVariant).toBe(RAW_CONFIG.variant);
    expect(params.variant).toBe(params.shippedVariant);
  });

  it("ships whichever variant the config names", () => {
    const dir = mkdtempSync(join(tmpdir(), "jotra-app-icon-config-"));
    try {
      const configPath = join(dir, "app-icon.json");
      const [name] = otherVariants;
      writeFileSync(configPath, JSON.stringify({ ...RAW_CONFIG, variant: name }));
      const switched = loadIconParams({ configPath });
      expect(switched.shippedVariant).toBe(name);
      expect(switched.variant).toBe(name);
      expect(switched.primitives).toEqual(loadIconParams({ variant: name }).primitives);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("rejects an unknown variant with IconParamsError", () => {
    expect(() => loadIconParams({ variant: "j-missing" })).toThrow(/unknown icon variant j-missing/);
    expect(() => loadIconParams({ variant: "J Dot" })).toThrow(IconParamsError);
  });

  it("rejects invalid config with IconParamsError", () => {
    const invalid = [
      mutate(RAW_CONFIG, (raw) => (raw.tile.color = "#09B13")),
      mutate(RAW_CONFIG, (raw) => (raw.tile.superellipseExponent = 1)),
      mutate(RAW_CONFIG, (raw) => (raw.tile.superellipseExponent = 4.5)),
      mutate(RAW_CONFIG, (raw) => (raw.tile.body.flat = 1100)),
      mutate(RAW_CONFIG, (raw) => (raw.variant = "J Dot")),
      mutate(RAW_CONFIG, (raw) => delete raw.variant),
      mutate(RAW_CONFIG, (raw) => (raw.glyph.points = [[512, 512]])),
      mutate(RAW_CONFIG, (raw) => (raw.extra = true)),
    ];
    for (const raw of invalid) expect(() => parseIconConfig(raw)).toThrow(IconParamsError);
  });

  it("rejects invalid variants with IconParamsError", () => {
    const invalid = [
      mutate(SYNTHETIC_VARIANT, (raw) => (raw.stroke.widthForm = "ios")),
      mutate(SYNTHETIC_VARIANT, (raw) => (raw.stroke.smallSizes = [[32, 2], [16, 1.5]])),
      mutate(SYNTHETIC_VARIANT, (raw) => (raw.stroke.proportionalFrom = 64)),
      mutate(SYNTHETIC_VARIANT, (raw) => (raw.primitives = [])),
      mutate(SYNTHETIC_VARIANT, (raw) => (raw.primitives[0].points = [[512, 400]])),
      mutate(SYNTHETIC_VARIANT, (raw) => (raw.primitives[1].sweepDegrees = 0)),
      mutate(SYNTHETIC_VARIANT, (raw) => (raw.primitives[1].sweepDegrees = 360)),
      mutate(SYNTHETIC_VARIANT, (raw) => (raw.primitives[1].radius = -5)),
      mutate(SYNTHETIC_VARIANT, (raw) => (raw.primitives[2].id = "stem")),
      mutate(SYNTHETIC_VARIANT, (raw) => (raw.primitives[2].id = "glyph")),
      mutate(SYNTHETIC_VARIANT, (raw) => (raw.primitives[2].type = "spline")),
      mutate(SYNTHETIC_VARIANT, (raw) => Object.assign(raw.primitives[2], { radius: 4 })),
      mutate(SYNTHETIC_VARIANT, (raw) => Object.assign(raw.optical[0], { target: "tail" })),
      mutate(SYNTHETIC_VARIANT, (raw) => Object.assign(raw.optical[0], { diameterPx: 2 })),
      mutate(SYNTHETIC_VARIANT, (raw) => Object.assign(raw.optical[1], { target: "stem" })),
      mutate(SYNTHETIC_VARIANT, (raw) => raw.optical.push({ ...raw.optical[0], translatePx: [0, 1] })),
      mutate(SYNTHETIC_VARIANT, (raw) => (raw.optical[0].sizes = [16, 16])),
      mutate(SYNTHETIC_VARIANT, (raw) => (raw.optical[0].forms = ["ios"])),
      mutate(SYNTHETIC_VARIANT, (raw) => raw.optical.push({ target: "glyph", forms: ["flat"], sizes: [32] })),
      mutate(SYNTHETIC_VARIANT, (raw) => Object.assign(raw, { glyph: {} })),
    ];
    for (const raw of invalid) expect(() => parseIconVariant(raw, "synthetic")).toThrow(IconParamsError);
    expect(() => parseIconVariant(SYNTHETIC_VARIANT, "Synthetic")).toThrow(IconParamsError);
  });

  it.each(variantNames)("lays out %s inside the tile at every shipped size and renders the small sizes", (name) => {
    const variant = loadIconParams({ variant: name });
    for (const form of ICON_FORMS) {
      for (const size of SHIPPED_SIZES[form]) {
        const layout = layoutIcon(variant, form, size);
        expect(layout.primitives.map(({ id }) => id)).toEqual(variant.primitives.map(({ id }) => id));
        if (size <= 64) expect(renderIcon(variant, form, size).width).toBe(size);
      }
    }
  });
});

describe("app icon geometry", () => {
  it("snaps each body to whole-pixel margins", () => {
    const bodies = (form: IconForm, sizes: number[]) => sizes.map((size) => computeBodyPx(params, form, size));
    expect(bodies("macos", [16, 32, 64, 128, 256, 1024])).toEqual([12, 26, 52, 102, 206, 824]);
    expect(bodies("flat", [16, 24, 32, 48, 64, 256, 1024])).toEqual([14, 20, 28, 42, 56, 224, 896]);
  });

  it("uses the small-size stroke anchors and scales the stroke with the body from 128", () => {
    const regular = loadIconParams({ variant: "j-dot" });
    expect([16, 32, 64].map((size) => computeStrokePx(regular, "macos", size))).toEqual([1.5, 2, 3]);
    expect(computeStrokePx(regular, "flat", 16)).toBe(1.5);
    expect(computeStrokePx(regular, "macos", 48)).toBe(2.5);
    expect(computeStrokePx(regular, "macos", 1024)).toBe(37);
    expect(computeStrokePx(regular, "macos", 128)).toBeCloseTo((37 * 102) / 824, 10);
    expect(computeStrokePx(regular, "flat", 1024)).toBeCloseTo((37 * 896) / 824, 10);

    const bold = loadIconParams({ variant: "j-dot-bold" });
    expect([16, 32, 64].map((size) => computeStrokePx(bold, "macos", size))).toEqual([2, 2.5, 4]);
    expect(computeStrokePx(bold, "macos", 1024)).toBe(52);
    expect(computeStrokePx(bold, "macos", 128)).toBeCloseTo((52 * 102) / 824, 10);
  });

  it("applies optical entries only at their own form and size", () => {
    const tuned = parseSynthetic();
    const plain = parseSynthetic((raw) => (raw.optical = []));
    const pair = (form: IconForm, size: number) => [layoutIcon(tuned, form, size), layoutIcon(plain, form, size)];

    const [macos16, plainMacos16] = pair("macos", 16);
    expectShift(readAnchor(findPrimitive(macos16, "stem")), readAnchor(findPrimitive(plainMacos16, "stem")), [0.5, -0.25]);
    expectShift(readAnchor(findPrimitive(macos16, "hook")), readAnchor(findPrimitive(plainMacos16, "hook")), [0.5, -0.25]);
    expectShift(readAnchor(findPrimitive(macos16, "dot")), readAnchor(findPrimitive(plainMacos16, "dot")), [0.5, -1.25]);
    expect(findPrimitive(macos16, "dot").reach).toBe(1.5);
    expect(findPrimitive(macos16, "stem").reach).toBe(0.75);

    const [flat16, plainFlat16] = pair("flat", 16);
    expectShift(readAnchor(findPrimitive(flat16, "stem")), readAnchor(findPrimitive(plainFlat16, "stem")), [0, 0]);
    expectShift(readAnchor(findPrimitive(flat16, "dot")), readAnchor(findPrimitive(plainFlat16, "dot")), [0, -1]);
    expect(findPrimitive(flat16, "dot").reach).toBe(1.5);

    const [macos32, plainMacos32] = pair("macos", 32);
    expect(macos32).toEqual(plainMacos32);
    expect(findPrimitive(macos32, "dot").reach).toBe(2);
  });

  it("refuses a glyph that leaves the tile, including through an optical shift", () => {
    const escaped = parseSynthetic((raw) => (raw.primitives[2].center = [60, 60]));
    expect(() => layoutIcon(escaped, "macos", 64)).toThrow(IconParamsError);
    expect(() => renderIcon(escaped, "macos", 64)).toThrow(IconParamsError);

    const pushed = parseSynthetic((raw) => (raw.optical[0].translatePx = [8, 0]));
    expect(() => layoutIcon(pushed, "macos", 16)).toThrow(/leaves the macos tile at 16 px/);
    expect(() => layoutIcon(pushed, "macos", 32)).not.toThrow();
  });

  it("rejects unknown forms and sizes with IconRenderError", () => {
    expect(() => renderIcon(params, "ios" as IconForm, 64)).toThrow(IconRenderError);
    expect(() => renderIcon(params, "macos", 0)).toThrow(IconRenderError);
    expect(() => renderIcon(params, "macos", 12.5)).toThrow(IconRenderError);
  });

  it.each(variantNames)("paints %s with a transparent margin, the tile colour and the glyph colour", (name) => {
    const variant = loadIconParams({ variant: name });
    const image = renderIcon(variant, "macos", 256);
    expect(readPixel(image, 0, 0)).toEqual([0, 0, 0, 0]);
    expect(readPixel(image, 24, 128)[3]).toBe(0);
    expect(readPixel(image, 25, 128)[3]).toBe(255);
    expect(readPixel(image, 40, 128)).toEqual([...variant.tileRgb, 255]);
    for (const primitive of layoutIcon(variant, "macos", 256).primitives) {
      const [x, y] = readCentrelinePoint(primitive);
      expect(readPixel(image, Math.floor(x), Math.floor(y)), primitive.id).toEqual([...variant.glyphRgb, 255]);
    }
  });
});

describe("app icon SVG", () => {
  it("writes polylines and arcs as one round-capped stroke path and dots as circles", () => {
    const svg = buildIconSvg(parseSynthetic(), "macos");
    expect(svg).toContain('viewBox="0 0 1024 1024"');
    expect(svg).toContain(`fill="${params.tileHex}"`);
    expect(svg).toContain(`stroke="${params.glyphHex}" stroke-width="40"`);
    expect(svg).toContain('stroke-linecap="round" stroke-linejoin="round"');
    expect(svg).toContain('d="M512 421.88L512 582.81A64.38 64.38 0 0 1 383.25 582.81"');
    expect(svg).toContain(`<circle fill="${params.glyphHex}" cx="512" cy="341.41" r="40"/>`);
  });

  it("sets the large-arc and sweep flags from the sweep angle", () => {
    const svg = buildIconSvg(
      parseSynthetic((raw) => {
        raw.primitives = [{ id: "ring", type: "arc", center: [512, 512], radius: 200, startDegrees: 90, sweepDegrees: -270 }];
        raw.optical = [];
      }),
      "macos",
    );
    expect(svg).toMatch(/d="M[\d.]+ [\d.]+A160\.94 160\.94 0 1 0 [\d.]+ [\d.]+"/);
    expect(svg).not.toContain("<circle");
  });

  it("writes the shipped variant's stroke at its full width", () => {
    const width = computeStrokePx(params, "macos", params.canvas);
    expect(buildIconSvg(params, "macos")).toContain(`stroke-width="${Math.round(width * 100) / 100}"`);
  });
});

describe("app icon build command", () => {
  it("parses --out and --variant", () => {
    expect(parseBuildArgs([])).toEqual({ variant: null, outRoot: ROOT });
    expect(parseBuildArgs(["--out", "icons-out"])).toEqual({ variant: null, outRoot: resolve("icons-out") });
    expect(parseBuildArgs(["--variant", "j-cap", "--out", "icons-out"])).toEqual({
      variant: "j-cap",
      outRoot: resolve("icons-out"),
    });
    const invalid = [["--variant", "j-cap"], ["--out", ""], ["--out"], ["--size", "16"], ["j-cap"]];
    for (const argv of invalid) expect(() => parseBuildArgs(argv)).toThrow(IconBuildArgsError);
  });

  it("recognises the repository root", () => {
    expect(isRepositoryRoot(ROOT)).toBe(true);
    expect(isRepositoryRoot(join(ROOT, "scripts", ".."))).toBe(true);
    expect(isRepositoryRoot(join(ROOT, "scripts"))).toBe(false);
  });

  it("refuses to write a variant other than the shipped one into the repository", () => {
    const before = readTrackedFiles();
    for (const name of otherVariants) {
      expect(() => runAppIconBuild(["--variant", name, "--out", ROOT])).toThrow(IconBuildArgsError);
    }
    expect(readTrackedFiles()).toEqual(before);
  });

  it("reports an unknown variant before writing anything", () => {
    const outRoot = join(tmpdir(), `jotra-app-icon-missing-${process.pid}`);
    expect(() => runAppIconBuild(["--variant", "j-missing", "--out", outRoot])).toThrow(IconParamsError);
    expect(existsSync(outRoot)).toBe(false);
  });

  it("exits 1 with the error name on bad arguments", () => {
    const result = spawnSync(process.execPath, [join(ROOT, "scripts/build-app-icon.mjs"), "--variant", "j-cap"], {
      cwd: ROOT,
      encoding: "utf8",
    });
    expect(result.status).toBe(1);
    expect(result.stderr).toMatch(/^IconBuildArgsError: --variant j-cap needs --out <dir>/);
  });

  it("renders another variant into --out and leaves the tracked files alone", () => {
    const [name] = otherVariants;
    const outRoot = mkdtempSync(join(tmpdir(), "jotra-app-icon-variant-"));
    try {
      const before = readTrackedFiles();
      const result = spawnSync(
        process.execPath,
        [join(ROOT, "scripts/build-app-icon.mjs"), "--variant", name, "--out", outRoot],
        { cwd: ROOT, encoding: "utf8" },
      );
      expect(result.stderr).toBe("");
      expect(result.status).toBe(0);
      for (const { path } of APP_ICON_FILES) expect(existsSync(join(outRoot, path)), path).toBe(true);
      const variant = loadIconParams({ variant: name });
      for (const { path, kind, form, size } of APP_ICON_FILES) {
        if (kind !== "png" || size === undefined) continue;
        expect(findPixelMismatch(renderIcon(variant, form, size), decodePng(readFileSync(join(outRoot, path)))), path).toBeNull();
      }
      expect(readFileSync(join(outRoot, "design/icon/app-icon.svg"), "utf8")).toBe(buildIconSvg(variant, "macos"));
      expect(readTrackedFiles()).toEqual(before);
    } finally {
      rmSync(outRoot, { recursive: true, force: true });
    }
  }, 120_000);
});

describe("app icon containers", () => {
  it("round-trips RGBA pixels through PNG", () => {
    const image = createPatternImage(19);
    expect(findPixelMismatch(image, decodePng(encodePng(image)))).toBeNull();
  });

  it("rejects a PNG whose chunk CRC is wrong", () => {
    const bytes = Buffer.from(encodePng(createPatternImage(4)));
    bytes[29] ^= 0xff;
    expect(() => decodePng(bytes)).toThrow(IconFormatError);
  });

  it("round-trips channels through ICNS RLE, including runs longer than one packet", () => {
    const pattern = createPatternImage(16);
    const decoded = decodeIcnsRle(encodeIcnsRle(pattern), 256);
    for (let index = 0; index < 256; index++) {
      expect(Array.from(decoded.subarray(index * 3, index * 3 + 3))).toEqual(
        Array.from(pattern.rgba.subarray(index * 4, index * 4 + 3)),
      );
    }
    const flat: RasterImage = { width: 32, height: 32, rgba: new Uint8Array(32 * 32 * 4).fill(9) };
    expect(decodeIcnsRle(encodeIcnsRle(flat), 1024).every((value) => value === 9)).toBe(true);
    expect(() => decodeIcnsRle(encodeIcnsRle(flat).subarray(0, 5), 1024)).toThrow(IconFormatError);
  });

  it("writes ICO directories with PNG frames and the 256 width as zero", () => {
    const images = [16, 256].map(createPatternImage);
    const bytes = encodeIco(images);
    expect(bytes[6 + 16]).toBe(0);
    const entries = parseIco(bytes);
    expect(entries.map(({ width, height, planes, bitCount }) => [width, height, planes, bitCount])).toEqual([
      [16, 16, 0, 32],
      [256, 256, 0, 32],
    ]);
    entries.forEach((entry, index) => expect(findPixelMismatch(images[index], decodePng(entry.data))).toBeNull());
  });

  it("writes ICNS elements in layout order with RLE masks", () => {
    const images = new Map(ICNS_LAYOUT.map(({ size }) => [size, createPatternImage(size)]));
    const elements = parseIcns(encodeIcns(images));
    expect(elements.map(({ type }) => type)).toEqual(
      ICNS_LAYOUT.flatMap(({ type, mask }) => (mask === undefined ? [type] : [type, mask])),
    );
    for (const { type, size } of ICNS_LAYOUT) {
      expect(findPixelMismatch(createPatternImage(size), decodeIcnsImage(elements, type)), type).toBeNull();
    }
  });
});

describe("committed app icon files", () => {
  let outRoot = "";

  beforeAll(() => {
    outRoot = mkdtempSync(join(tmpdir(), "jotra-app-icon-"));
    writeAppIconFiles(outRoot, buildAppIconFiles(params));
  }, 120_000);

  afterAll(() => {
    if (outRoot) rmSync(outRoot, { recursive: true, force: true });
  });

  const readCommitted = (path: string) => readFileSync(join(ROOT, path));
  const readFresh = (path: string) => readFileSync(join(outRoot, path));
  const fileOfKind = (kind: string) => {
    const file = APP_ICON_FILES.find((candidate) => candidate.kind === kind);
    if (file === undefined) throw new Error(`APP_ICON_FILES has no ${kind} entry`);
    return file;
  };

  it.each(APP_ICON_FILES.filter(({ kind }) => kind === "png"))("$path matches a fresh render", ({ path, size }) => {
    const committed = decodePng(readCommitted(path));
    expect(committed.width).toBe(size);
    expect(findPixelMismatch(decodePng(readFresh(path)), committed), path).toBeNull();
  });

  it.each(otherVariants)("the committed rasters differ from %s", (name) => {
    const file = APP_ICON_FILES.find(({ kind, size }) => kind === "png" && size === 128);
    if (file === undefined || file.size === undefined) throw new Error("APP_ICON_FILES has no 128 px PNG");
    const committed = decodePng(readCommitted(file.path));
    expect(findPixelMismatch(renderIcon(loadIconParams({ variant: name }), file.form, file.size), committed)).not.toBeNull();
  });

  it("icon.ico holds the expected PNG frames and matches a fresh render", () => {
    const { path } = fileOfKind("ico");
    const committed = parseIco(readCommitted(path));
    const fresh = parseIco(readFresh(path));
    expect(committed.map(({ width, height, planes, bitCount }) => [width, height, planes, bitCount])).toEqual(
      ICO_SIZES.map((size) => [size, size, 0, 32]),
    );
    expect(fresh).toHaveLength(committed.length);
    committed.forEach((entry, index) => {
      const image = decodePng(entry.data);
      expect(image.width).toBe(entry.width);
      expect(findPixelMismatch(decodePng(fresh[index].data), image), `${path} frame ${entry.width}`).toBeNull();
    });
  });

  it("icon.icns holds the expected elements and matches a fresh render", () => {
    const { path } = fileOfKind("icns");
    const committed = parseIcns(readCommitted(path));
    const fresh = parseIcns(readFresh(path));
    const expectedTypes = ICNS_LAYOUT.flatMap(({ type, mask }) => (mask === undefined ? [type] : [type, mask]));
    expect(committed.map(({ type }) => type)).toEqual(expectedTypes);
    expect(fresh.map(({ type }) => type)).toEqual(expectedTypes);
    for (const { type, size } of ICNS_LAYOUT) {
      const image = decodeIcnsImage(committed, type);
      expect(image.width).toBe(size);
      expect(findPixelMismatch(decodeIcnsImage(fresh, type), image), `${path} ${type}`).toBeNull();
    }
  });

  it("app-icon.svg matches a fresh build", () => {
    const { path } = fileOfKind("svg");
    expect(readCommitted(path).toString("utf8")).toBe(readFresh(path).toString("utf8"));
  });
});
