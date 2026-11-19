import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
export const PARAMS_PATH = resolve(ROOT, "design/icon/app-icon.json");
export const VARIANTS_DIR = resolve(ROOT, "design/icon/variants");
export const ICON_FORMS = Object.freeze(["macos", "flat"]);
export const PRIMITIVE_TYPES = Object.freeze(["polyline", "arc", "dot"]);
export const GLYPH_TARGET = "glyph";

const SUBSAMPLES = 16;
const OUTLINE_STEPS_PER_OCTANT = 64;
const HEX_COLOR = /^#[0-9A-Fa-f]{6}$/;
const VARIANT_NAME = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const PRIMITIVE_ID = /^[a-z][a-z0-9-]*$/;
const OUTSIDE = 0;
const PARTIAL = 1;
const INSIDE = 2;

export class IconParamsError extends Error {
  constructor(message) {
    super(message);
    this.name = "IconParamsError";
  }
}

export class IconRenderError extends Error {
  constructor(message) {
    super(message);
    this.name = "IconRenderError";
  }
}

function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function requireRecord(value, label) {
  if (!isRecord(value)) throw new IconParamsError(`${label} must be an object`);
  return value;
}

function rejectUnknownKeys(record, allowed, label) {
  const unknown = Object.keys(record).filter((key) => !allowed.includes(key));
  if (unknown.length > 0) {
    throw new IconParamsError(`${label} has unknown keys ${unknown.join(", ")}; allowed: ${allowed.join(", ")}`);
  }
}

function requireNumber(value, label, { integer = false, positive = false } = {}) {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new IconParamsError(`${label} must be a finite number, got ${JSON.stringify(value)}`);
  }
  if (integer && !Number.isInteger(value)) throw new IconParamsError(`${label} must be an integer, got ${value}`);
  if (positive && value <= 0) throw new IconParamsError(`${label} must be greater than 0, got ${value}`);
  return value;
}

function requireArray(value, label, minLength) {
  if (!Array.isArray(value) || value.length < minLength) {
    throw new IconParamsError(`${label} must be an array with at least ${minLength} item${minLength === 1 ? "" : "s"}`);
  }
  return value;
}

function parseHexColor(value, label) {
  if (typeof value !== "string" || !HEX_COLOR.test(value)) {
    throw new IconParamsError(`${label} must be a #RRGGBB colour, got ${JSON.stringify(value)}`);
  }
  const rgb = [1, 3, 5].map((start) => Number.parseInt(value.slice(start, start + 2), 16));
  return { hex: value.toUpperCase(), rgb: Object.freeze(rgb) };
}

function parsePair(value, label, options) {
  if (!Array.isArray(value) || value.length !== 2) throw new IconParamsError(`${label} must be a [number, number] pair`);
  return Object.freeze([requireNumber(value[0], `${label}[0]`, options), requireNumber(value[1], `${label}[1]`, options)]);
}

function parseForm(value, label) {
  if (!ICON_FORMS.includes(value)) {
    throw new IconParamsError(`${label} must be one of ${ICON_FORMS.join(", ")}, got ${JSON.stringify(value)}`);
  }
  return value;
}

function parseVariantName(value, label) {
  if (typeof value !== "string" || !VARIANT_NAME.test(value)) {
    throw new IconParamsError(`${label} must be a lowercase name such as j-dot, got ${JSON.stringify(value)}`);
  }
  return value;
}

export function parseIconConfig(raw) {
  const root = requireRecord(raw, "app icon config");
  rejectUnknownKeys(root, ["canvas", "variant", "tile", "glyph"], "app icon config");
  const canvas = requireNumber(root.canvas, "canvas", { integer: true, positive: true });
  const variant = parseVariantName(root.variant, "variant");
  const tile = requireRecord(root.tile, "tile");
  rejectUnknownKeys(tile, ["color", "superellipseExponent", "body"], "tile");
  const glyph = requireRecord(root.glyph, "glyph");
  rejectUnknownKeys(glyph, ["color", "frame"], "glyph");

  const exponent = requireNumber(tile.superellipseExponent, "tile.superellipseExponent", { integer: true });
  if (exponent < 2) throw new IconParamsError(`tile.superellipseExponent must be at least 2, got ${exponent}`);

  const bodyRecord = requireRecord(tile.body, "tile.body");
  rejectUnknownKeys(bodyRecord, ICON_FORMS, "tile.body");
  const bodies = {};
  for (const form of ICON_FORMS) {
    const body = requireNumber(bodyRecord[form], `tile.body.${form}`, { positive: true });
    if (body > canvas) throw new IconParamsError(`tile.body.${form} (${body}) exceeds the canvas (${canvas})`);
    bodies[form] = body;
  }

  const tileColor = parseHexColor(tile.color, "tile.color");
  const glyphColor = parseHexColor(glyph.color, "glyph.color");
  return Object.freeze({
    canvas,
    variant,
    exponent,
    bodies: Object.freeze(bodies),
    tileHex: tileColor.hex,
    tileRgb: tileColor.rgb,
    glyphHex: glyphColor.hex,
    glyphRgb: glyphColor.rgb,
    frame: requireNumber(glyph.frame, "glyph.frame", { positive: true }),
  });
}

function parseStroke(raw, label) {
  const stroke = requireRecord(raw, label);
  rejectUnknownKeys(stroke, ["width", "widthForm", "smallSizes", "proportionalFrom"], label);
  const strokeWidth = requireNumber(stroke.width, `${label}.width`, { positive: true });
  const strokeWidthForm = parseForm(stroke.widthForm, `${label}.widthForm`);
  const smallSizeStrokes = requireArray(stroke.smallSizes, `${label}.smallSizes`, 1).map((pair, index) =>
    parsePair(pair, `${label}.smallSizes[${index}]`, { positive: true }),
  );
  smallSizeStrokes.forEach(([size], index) => {
    if (!Number.isInteger(size)) throw new IconParamsError(`${label}.smallSizes[${index}] size must be an integer`);
    if (index > 0 && size <= smallSizeStrokes[index - 1][0]) {
      throw new IconParamsError(`${label}.smallSizes sizes must be strictly increasing`);
    }
  });
  const proportionalStrokeFrom = requireNumber(stroke.proportionalFrom, `${label}.proportionalFrom`, {
    integer: true,
    positive: true,
  });
  if (proportionalStrokeFrom <= smallSizeStrokes[smallSizeStrokes.length - 1][0]) {
    throw new IconParamsError(`${label}.proportionalFrom must be larger than the last small-size stroke`);
  }
  return {
    strokeWidth,
    strokeWidthForm,
    smallSizeStrokes: Object.freeze(smallSizeStrokes),
    proportionalStrokeFrom,
  };
}

function parsePrimitive(raw, label) {
  const primitive = requireRecord(raw, label);
  if (typeof primitive.id !== "string" || !PRIMITIVE_ID.test(primitive.id)) {
    throw new IconParamsError(`${label}.id must be a lowercase identifier, got ${JSON.stringify(primitive.id)}`);
  }
  if (primitive.id === GLYPH_TARGET) throw new IconParamsError(`${label}.id must not be ${GLYPH_TARGET}`);
  const { id, type } = primitive;
  switch (type) {
    case "polyline":
      rejectUnknownKeys(primitive, ["id", "type", "points"], label);
      return Object.freeze({
        id,
        type,
        points: Object.freeze(
          requireArray(primitive.points, `${label}.points`, 2).map((point, index) =>
            parsePair(point, `${label}.points[${index}]`),
          ),
        ),
      });
    case "arc": {
      rejectUnknownKeys(primitive, ["id", "type", "center", "radius", "startDegrees", "sweepDegrees"], label);
      const sweepDegrees = requireNumber(primitive.sweepDegrees, `${label}.sweepDegrees`);
      if (sweepDegrees === 0 || Math.abs(sweepDegrees) >= 360) {
        throw new IconParamsError(`${label}.sweepDegrees must be non-zero and below 360 in magnitude, got ${sweepDegrees}`);
      }
      return Object.freeze({
        id,
        type,
        center: parsePair(primitive.center, `${label}.center`),
        radius: requireNumber(primitive.radius, `${label}.radius`, { positive: true }),
        startDegrees: requireNumber(primitive.startDegrees, `${label}.startDegrees`),
        sweepDegrees,
      });
    }
    case "dot":
      rejectUnknownKeys(primitive, ["id", "type", "center", "diameterInStrokes"], label);
      return Object.freeze({
        id,
        type,
        center: parsePair(primitive.center, `${label}.center`),
        diameterInStrokes: requireNumber(primitive.diameterInStrokes, `${label}.diameterInStrokes`, { positive: true }),
      });
    default:
      throw new IconParamsError(`${label}.type must be one of ${PRIMITIVE_TYPES.join(", ")}, got ${JSON.stringify(type)}`);
  }
}

function parseOpticalEntry(raw, primitivesById, label) {
  const entry = requireRecord(raw, label);
  rejectUnknownKeys(entry, ["target", "sizes", "forms", "translatePx", "diameterPx"], label);
  const { target } = entry;
  if (target !== GLYPH_TARGET && !primitivesById.has(target)) {
    throw new IconParamsError(`${label}.target must be ${GLYPH_TARGET} or a primitive id, got ${JSON.stringify(target)}`);
  }
  const sizes = requireArray(entry.sizes, `${label}.sizes`, 1).map((size, index) =>
    requireNumber(size, `${label}.sizes[${index}]`, { integer: true, positive: true }),
  );
  if (new Set(sizes).size !== sizes.length) throw new IconParamsError(`${label}.sizes lists a size twice`);
  const forms =
    entry.forms === undefined
      ? ICON_FORMS
      : requireArray(entry.forms, `${label}.forms`, 1).map((form, index) => parseForm(form, `${label}.forms[${index}]`));
  if (new Set(forms).size !== forms.length) throw new IconParamsError(`${label}.forms lists a form twice`);
  if (entry.translatePx === undefined && entry.diameterPx === undefined) {
    throw new IconParamsError(`${label} must set translatePx, diameterPx or both`);
  }
  const translatePx = entry.translatePx === undefined ? Object.freeze([0, 0]) : parsePair(entry.translatePx, `${label}.translatePx`);
  let diameterPx = null;
  if (entry.diameterPx !== undefined) {
    if (primitivesById.get(target)?.type !== "dot") throw new IconParamsError(`${label}.diameterPx applies to dot targets only`);
    diameterPx = requireNumber(entry.diameterPx, `${label}.diameterPx`, { positive: true });
  }
  return Object.freeze({
    target,
    sizes: Object.freeze(sizes),
    forms: Object.freeze([...forms]),
    translatePx,
    diameterPx,
  });
}

export function parseIconVariant(raw, name) {
  const variantName = parseVariantName(name, "variant name");
  const label = `variant ${variantName}`;
  const root = requireRecord(raw, label);
  rejectUnknownKeys(root, ["stroke", "primitives", "optical"], label);
  const stroke = parseStroke(root.stroke, `${label} stroke`);

  const primitives = requireArray(root.primitives, `${label} primitives`, 1).map((primitive, index) =>
    parsePrimitive(primitive, `${label} primitives[${index}]`),
  );
  const primitivesById = new Map();
  for (const primitive of primitives) {
    if (primitivesById.has(primitive.id)) throw new IconParamsError(`${label} has two primitives with id ${primitive.id}`);
    primitivesById.set(primitive.id, primitive);
  }

  const optical = (root.optical === undefined ? [] : requireArray(root.optical, `${label} optical`, 0)).map(
    (entry, index) => parseOpticalEntry(entry, primitivesById, `${label} optical[${index}]`),
  );
  const claimed = new Set();
  for (const entry of optical) {
    for (const size of entry.sizes) {
      for (const form of entry.forms) {
        const key = `${entry.target} ${form} ${size}`;
        if (claimed.has(key)) throw new IconParamsError(`${label} optical adjusts ${entry.target} twice at ${form} ${size} px`);
        claimed.add(key);
      }
    }
  }

  return Object.freeze({ name: variantName, ...stroke, primitives: Object.freeze(primitives), optical: Object.freeze(optical) });
}

export function parseIconParams(configRaw, variantRaw, variantName) {
  const config = parseIconConfig(configRaw);
  const variant = parseIconVariant(variantRaw, variantName ?? config.variant);
  return Object.freeze({
    canvas: config.canvas,
    exponent: config.exponent,
    bodies: config.bodies,
    tileHex: config.tileHex,
    tileRgb: config.tileRgb,
    glyphHex: config.glyphHex,
    glyphRgb: config.glyphRgb,
    frame: config.frame,
    shippedVariant: config.variant,
    variant: variant.name,
    strokeWidth: variant.strokeWidth,
    strokeWidthForm: variant.strokeWidthForm,
    smallSizeStrokes: variant.smallSizeStrokes,
    proportionalStrokeFrom: variant.proportionalStrokeFrom,
    primitives: variant.primitives,
    optical: variant.optical,
  });
}

function readJsonFile(path, label) {
  let text;
  try {
    text = readFileSync(path, "utf8");
  } catch (error) {
    throw new IconParamsError(`cannot read ${label} at ${path}: ${error.message}`);
  }
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new IconParamsError(`${label} at ${path} is not valid JSON: ${error.message}`);
  }
}

export function listIconVariants(variantsDir = VARIANTS_DIR) {
  let entries;
  try {
    entries = readdirSync(variantsDir);
  } catch (error) {
    throw new IconParamsError(`cannot list icon variants in ${variantsDir}: ${error.message}`);
  }
  return entries
    .filter((entry) => entry.endsWith(".json"))
    .map((entry) => entry.slice(0, -".json".length))
    .filter((name) => VARIANT_NAME.test(name))
    .sort();
}

export function loadIconParams({ variant, configPath = PARAMS_PATH, variantsDir = VARIANTS_DIR } = {}) {
  const configRaw = readJsonFile(configPath, "app icon config");
  const name = parseVariantName(variant ?? parseIconConfig(configRaw).variant, "variant");
  const variantPath = resolve(variantsDir, `${name}.json`);
  if (!existsSync(variantPath)) {
    throw new IconParamsError(`unknown icon variant ${name}; ${variantsDir} has ${listIconVariants(variantsDir).join(", ")}`);
  }
  return parseIconParams(configRaw, readJsonFile(variantPath, `icon variant ${name}`), name);
}

function requireBody(params, form) {
  if (!ICON_FORMS.includes(form)) {
    throw new IconRenderError(`unknown icon form ${JSON.stringify(form)}; expected one of ${ICON_FORMS.join(", ")}`);
  }
  return params.bodies[form];
}

function requireSize(size) {
  if (!Number.isInteger(size) || size < 1) throw new IconRenderError(`icon size must be a positive integer, got ${size}`);
}

export function computeBodyPx(params, form, size) {
  const body = requireBody(params, form);
  requireSize(size);
  const margin = Math.round((size * (params.canvas - body)) / (2 * params.canvas));
  return size - 2 * margin;
}

export function computeStrokePx(params, form, size) {
  const strokePerBodyPx = params.strokeWidth / params.bodies[params.strokeWidthForm];
  const proportional = (at) => strokePerBodyPx * computeBodyPx(params, form, at);
  if (size >= params.proportionalStrokeFrom) return proportional(size);
  requireSize(size);
  const anchors = [...params.smallSizeStrokes, [params.proportionalStrokeFrom, proportional(params.proportionalStrokeFrom)]];
  if (size <= anchors[0][0]) return anchors[0][1];
  for (let index = 1; index < anchors.length; index++) {
    const [fromSize, fromWidth] = anchors[index - 1];
    const [toSize, toWidth] = anchors[index];
    if (size <= toSize) return fromWidth + ((size - fromSize) / (toSize - fromSize)) * (toWidth - fromWidth);
  }
  return proportional(size);
}

function powInteger(base, exponent) {
  let result = 1;
  for (let step = 0; step < exponent; step++) result *= base;
  return result;
}

function rootInteger(value, exponent) {
  if (value <= 0) return 0;
  let root = value > 1 ? value : 1;
  for (let step = 0; step < 200; step++) {
    const next = root - (powInteger(root, exponent) - value) / (exponent * powInteger(root, exponent - 1));
    if (next >= root) return root;
    root = next;
  }
  return root;
}

function computeUnitVector(degrees) {
  const turn = ((degrees % 360) + 360) % 360;
  if (turn === 0) return [1, 0];
  if (turn === 90) return [0, 1];
  if (turn === 180) return [-1, 0];
  if (turn === 270) return [0, -1];
  const radians = (turn * Math.PI) / 180;
  return [Math.cos(radians), Math.sin(radians)];
}

function computeArcPoint(arc, degrees) {
  const [ux, uy] = computeUnitVector(degrees);
  return [arc.center[0] + arc.radius * ux, arc.center[1] + arc.radius * uy];
}

function isAngleOnArc(arc, degrees) {
  const from = arc.sweepDegrees >= 0 ? arc.startDegrees : arc.startDegrees + arc.sweepDegrees;
  const offset = ((((degrees - from) % 360) + 360) % 360);
  return offset <= Math.abs(arc.sweepDegrees);
}

function listArcExtremes(arc) {
  const extremes = [computeArcPoint(arc, arc.startDegrees), computeArcPoint(arc, arc.startDegrees + arc.sweepDegrees)];
  for (const axis of [0, 90, 180, 270]) {
    if (isAngleOnArc(arc, axis)) extremes.push(computeArcPoint(arc, axis));
  }
  return extremes;
}

function isInsideTile(layout, x, y) {
  const u = Math.abs(x - layout.centre) / layout.halfBody;
  const v = Math.abs(y - layout.centre) / layout.halfBody;
  return powInteger(u, layout.exponent) + powInteger(v, layout.exponent) <= 1;
}

function listPrimitiveExtremes(primitive) {
  switch (primitive.type) {
    case "polyline":
      return primitive.points;
    case "arc":
      return listArcExtremes(primitive);
    default:
      return [primitive.center];
  }
}

function collectOpticalAdjustments(params, form, size) {
  const adjustments = new Map();
  for (const entry of params.optical) {
    if (entry.sizes.includes(size) && entry.forms.includes(form)) adjustments.set(entry.target, entry);
  }
  return adjustments;
}

export function layoutIcon(params, form, size) {
  const body = computeBodyPx(params, form, size);
  const strokePx = computeStrokePx(params, form, size);
  const strokeRadius = strokePx / 2;
  const centre = size / 2;
  const scale = body / params.frame;
  const frameCentre = params.frame / 2;
  const adjustments = collectOpticalAdjustments(params, form, size);
  const [glyphDx, glyphDy] = adjustments.get(GLYPH_TARGET)?.translatePx ?? [0, 0];

  const primitives = params.primitives.map((primitive) => {
    const adjustment = adjustments.get(primitive.id);
    const [dx, dy] = adjustment?.translatePx ?? [0, 0];
    const place = ([x, y]) =>
      Object.freeze([centre + (x - frameCentre) * scale + glyphDx + dx, centre + (y - frameCentre) * scale + glyphDy + dy]);
    switch (primitive.type) {
      case "polyline":
        return Object.freeze({
          id: primitive.id,
          type: primitive.type,
          points: Object.freeze(primitive.points.map(place)),
          reach: strokeRadius,
        });
      case "arc":
        return Object.freeze({
          id: primitive.id,
          type: primitive.type,
          center: place(primitive.center),
          radius: primitive.radius * scale,
          startDegrees: primitive.startDegrees,
          sweepDegrees: primitive.sweepDegrees,
          reach: strokeRadius,
        });
      default: {
        const diameter = adjustment?.diameterPx ?? primitive.diameterInStrokes * strokePx;
        return Object.freeze({ id: primitive.id, type: primitive.type, center: place(primitive.center), reach: diameter / 2 });
      }
    }
  });

  const layout = Object.freeze({
    size,
    centre,
    halfBody: body / 2,
    exponent: params.exponent,
    strokeRadius,
    primitives: Object.freeze(primitives),
  });
  for (const primitive of primitives) {
    const extremes = listPrimitiveExtremes(primitive);
    const r = primitive.reach;
    const xs = extremes.map(([x]) => x);
    const ys = extremes.map(([, y]) => y);
    const [left, right] = [Math.min(...xs) - r, Math.max(...xs) + r];
    const [top, bottom] = [Math.min(...ys) - r, Math.max(...ys) + r];
    const corners = [
      [left, top],
      [right, top],
      [left, bottom],
      [right, bottom],
    ];
    if (!corners.every(([x, y]) => isInsideTile(layout, x, y))) {
      throw new IconParamsError(`glyph primitive ${primitive.id} of variant ${params.variant} leaves the ${form} tile at ${size} px`);
    }
  }
  return layout;
}

function createSegmentShape([ax, ay], [bx, by], reach) {
  const dx = bx - ax;
  const dy = by - ay;
  const lengthSquared = dx * dx + dy * dy;
  return {
    reach,
    measureDistance(x, y) {
      let t = lengthSquared === 0 ? 0 : ((x - ax) * dx + (y - ay) * dy) / lengthSquared;
      t = t < 0 ? 0 : t > 1 ? 1 : t;
      const offsetX = ax + t * dx - x;
      const offsetY = ay + t * dy - y;
      return Math.sqrt(offsetX * offsetX + offsetY * offsetY);
    },
  };
}

function createArcShape(arc) {
  const [cx, cy] = arc.center;
  const { radius, reach } = arc;
  const span = Math.abs(arc.sweepDegrees);
  const from = arc.sweepDegrees >= 0 ? arc.startDegrees : arc.startDegrees + arc.sweepDegrees;
  const [fromX, fromY] = computeUnitVector(from);
  const [toX, toY] = computeUnitVector(from + span);
  const [endAX, endAY] = [cx + radius * fromX, cy + radius * fromY];
  const [endBX, endBY] = [cx + radius * toX, cy + radius * toY];
  const isInSector =
    span <= 180
      ? (vx, vy) => fromX * vy - fromY * vx >= 0 && vx * toY - vy * toX >= 0
      : (vx, vy) => !(toX * vy - toY * vx > 0 && vx * fromY - vy * fromX > 0);
  return {
    reach,
    measureDistance(x, y) {
      const vx = x - cx;
      const vy = y - cy;
      if (isInSector(vx, vy)) return Math.abs(Math.sqrt(vx * vx + vy * vy) - radius);
      const ax = x - endAX;
      const ay = y - endAY;
      const bx = x - endBX;
      const by = y - endBY;
      return Math.sqrt(Math.min(ax * ax + ay * ay, bx * bx + by * by));
    },
  };
}

function createDotShape(dot) {
  const [cx, cy] = dot.center;
  return {
    reach: dot.reach,
    measureDistance(x, y) {
      const dx = x - cx;
      const dy = y - cy;
      return Math.sqrt(dx * dx + dy * dy);
    },
  };
}

function buildShapes(layout) {
  const shapes = [];
  for (const primitive of layout.primitives) {
    switch (primitive.type) {
      case "polyline":
        for (let index = 1; index < primitive.points.length; index++) {
          shapes.push(createSegmentShape(primitive.points[index - 1], primitive.points[index], primitive.reach));
        }
        break;
      case "arc":
        shapes.push(createArcShape(primitive));
        break;
      default:
        shapes.push(createDotShape(primitive));
    }
  }
  return shapes;
}

function classifyTile(layout, px, py) {
  const { centre } = layout;
  const nearX = Math.min(Math.max(centre, px), px + 1);
  const nearY = Math.min(Math.max(centre, py), py + 1);
  if (!isInsideTile(layout, nearX, nearY)) return OUTSIDE;
  const farX = Math.abs(px - centre) > Math.abs(px + 1 - centre) ? px : px + 1;
  const farY = Math.abs(py - centre) > Math.abs(py + 1 - centre) ? py : py + 1;
  return isInsideTile(layout, farX, farY) ? INSIDE : PARTIAL;
}

function classifyGlyph(shapes, partial, px, py) {
  partial.length = 0;
  const x = px + 0.5;
  const y = py + 0.5;
  for (const shape of shapes) {
    const distance = shape.measureDistance(x, y);
    if (distance + Math.SQRT1_2 <= shape.reach) return INSIDE;
    if (distance - Math.SQRT1_2 <= shape.reach) partial.push(shape);
  }
  return partial.length > 0 ? PARTIAL : OUTSIDE;
}

function isInsideAnyShape(shapes, x, y) {
  for (const shape of shapes) {
    if (shape.measureDistance(x, y) <= shape.reach) return true;
  }
  return false;
}

export function renderIcon(params, form, size) {
  const layout = layoutIcon(params, form, size);
  const shapes = buildShapes(layout);
  const partial = [];
  const sampleCount = SUBSAMPLES * SUBSAMPLES;
  const tile = params.tileRgb;
  const glyph = params.glyphRgb;
  const rgba = new Uint8Array(size * size * 4);

  for (let py = 0; py < size; py++) {
    for (let px = 0; px < size; px++) {
      const tileState = classifyTile(layout, px, py);
      if (tileState === OUTSIDE) continue;
      const glyphState = classifyGlyph(shapes, partial, px, py);
      const offset = (py * size + px) * 4;

      if (tileState === INSIDE && glyphState !== PARTIAL) {
        const color = glyphState === INSIDE ? glyph : tile;
        rgba[offset] = color[0];
        rgba[offset + 1] = color[1];
        rgba[offset + 2] = color[2];
        rgba[offset + 3] = 255;
        continue;
      }

      let tileSamples = 0;
      let glyphSamples = 0;
      for (let sy = 0; sy < SUBSAMPLES; sy++) {
        const y = py + (sy + 0.5) / SUBSAMPLES;
        for (let sx = 0; sx < SUBSAMPLES; sx++) {
          const x = px + (sx + 0.5) / SUBSAMPLES;
          if (tileState !== INSIDE && !isInsideTile(layout, x, y)) continue;
          tileSamples++;
          if (glyphState === INSIDE || (glyphState === PARTIAL && isInsideAnyShape(partial, x, y))) glyphSamples++;
        }
      }
      if (tileSamples === 0) continue;
      const tileOnly = tileSamples - glyphSamples;
      for (let channel = 0; channel < 3; channel++) {
        rgba[offset + channel] = Math.round((glyphSamples * glyph[channel] + tileOnly * tile[channel]) / tileSamples);
      }
      rgba[offset + 3] = Math.round((255 * tileSamples) / sampleCount);
    }
  }
  return { width: size, height: size, rgba };
}

function formatCoordinate(value) {
  return String(Math.round(value * 100) / 100);
}

function formatPoint([x, y]) {
  return `${formatCoordinate(x)} ${formatCoordinate(y)}`;
}

function buildTileOutline(layout) {
  const { centre, halfBody, exponent } = layout;
  const diagonal = halfBody * rootInteger(0.5, exponent);
  const quadrant = [];
  for (let step = 0; step <= OUTLINE_STEPS_PER_OCTANT; step++) {
    const u = (diagonal * step) / OUTLINE_STEPS_PER_OCTANT;
    quadrant.push([u, halfBody * rootInteger(1 - powInteger(u / halfBody, exponent), exponent)]);
  }
  for (let step = OUTLINE_STEPS_PER_OCTANT - 1; step >= 0; step--) {
    const v = (diagonal * step) / OUTLINE_STEPS_PER_OCTANT;
    quadrant.push([halfBody * rootInteger(1 - powInteger(v / halfBody, exponent), exponent), v]);
  }
  const last = quadrant.length - 1;
  const loop = [
    ...quadrant,
    ...quadrant.slice(0, last).reverse().map(([u, v]) => [u, -v]),
    ...quadrant.slice(1).map(([u, v]) => [-u, -v]),
    ...quadrant.slice(1, last).reverse().map(([u, v]) => [-u, v]),
  ];
  return `M${loop.map(([u, v]) => formatPoint([centre + u, centre - v])).join("L")}Z`;
}

function buildStrokePath(primitives) {
  let path = "";
  let cursor = null;
  for (const primitive of primitives) {
    if (primitive.type === "polyline") {
      const [first, ...rest] = primitive.points.map(formatPoint);
      if (first !== cursor) path += `M${first}`;
      path += rest.map((point) => `L${point}`).join("");
      cursor = rest[rest.length - 1];
    } else if (primitive.type === "arc") {
      const start = formatPoint(computeArcPoint(primitive, primitive.startDegrees));
      const end = formatPoint(computeArcPoint(primitive, primitive.startDegrees + primitive.sweepDegrees));
      const radius = formatCoordinate(primitive.radius);
      const largeArc = Math.abs(primitive.sweepDegrees) > 180 ? 1 : 0;
      const sweep = primitive.sweepDegrees > 0 ? 1 : 0;
      if (start !== cursor) path += `M${start}`;
      path += `A${radius} ${radius} 0 ${largeArc} ${sweep} ${end}`;
      cursor = end;
    }
  }
  return path;
}

export function buildIconSvg(params, form) {
  const size = params.canvas;
  const layout = layoutIcon(params, form, size);
  const strokePath = buildStrokePath(layout.primitives);
  const lines = [
    `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${size} ${size}">`,
    `  <path fill="${params.tileHex}" d="${buildTileOutline(layout)}"/>`,
  ];
  if (strokePath !== "") {
    lines.push(
      `  <path fill="none" stroke="${params.glyphHex}" stroke-width="${formatCoordinate(layout.strokeRadius * 2)}" stroke-linecap="round" stroke-linejoin="round" d="${strokePath}"/>`,
    );
  }
  for (const primitive of layout.primitives) {
    if (primitive.type !== "dot") continue;
    const [cx, cy] = primitive.center;
    lines.push(
      `  <circle fill="${params.glyphHex}" cx="${formatCoordinate(cx)}" cy="${formatCoordinate(cy)}" r="${formatCoordinate(primitive.reach)}"/>`,
    );
  }
  lines.push("</svg>", "");
  return lines.join("\n");
}
