import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { ROOT, buildIconSvg, loadIconParams } from "../app-icon/render.mjs";

export const OG_IMAGE_PATH = "site/public/brand/og-1-0.png";
export const OG_FONT_PATH = "site/design-system/fonts/wix-madefor-display-latin-variable.woff2";
export const OG_TOKENS_PATH = "site/src/styles/tokens.css";
export const OG_WIDTH = 1200;
export const OG_HEIGHT = 630;
export const OG_NAME = "Jotra";
export const OG_FONT_FAMILY = "Wix Madefor Display";
export const OG_FONT_WEIGHT = 600;
export const OG_LOCKUP_SCALE = 7;

export const OG_SITE_TOKENS = Object.freeze({
  canvas: "--jotra-bg-canvas",
  fg: "--jotra-fg",
  iconPx: "--jotra-space-6",
  gapPx: "--jotra-space-3",
  fontPx: "--jotra-site-text-md",
});

const COLOR_TOKENS = new Set(["canvas", "fg"]);
const PNG_SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

export class OgImageInputError extends Error {
  constructor(message) {
    super(message);
    this.name = "OgImageInputError";
  }
}

export class OgImageRenderError extends Error {
  constructor(message) {
    super(message);
    this.name = "OgImageRenderError";
  }
}

export class OgImageOutputError extends Error {
  constructor(message) {
    super(message);
    this.name = "OgImageOutputError";
  }
}

function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function readTokenValue(css, token) {
  const matches = [...css.matchAll(new RegExp(`^\\s*${escapeRegExp(token)}\\s*:\\s*([^;]+);`, "gm"))];
  if (matches.length !== 1) {
    throw new OgImageInputError(`${OG_TOKENS_PATH} defines ${token} ${matches.length} times; the share image needs exactly one value`);
  }
  return matches[0][1].trim();
}

export function readSiteTokens(css) {
  const tokens = {};
  for (const [key, token] of Object.entries(OG_SITE_TOKENS)) {
    const value = readTokenValue(css, token);
    if (COLOR_TOKENS.has(key)) {
      if (!/^#[0-9a-fA-F]{6}$/.test(value)) {
        throw new OgImageInputError(`${token} is ${JSON.stringify(value)}; the share image needs a #RRGGBB colour`);
      }
      tokens[key] = value.toUpperCase();
    } else {
      const px = /^(\d+(?:\.\d+)?)px$/.exec(value);
      if (px === null || Number(px[1]) <= 0) {
        throw new OgImageInputError(`${token} is ${JSON.stringify(value)}; the share image needs a positive px length`);
      }
      tokens[key] = Number(px[1]);
    }
  }
  return Object.freeze(tokens);
}

function readInputFile(path, encoding) {
  const absolute = resolve(ROOT, path);
  if (!existsSync(absolute)) throw new OgImageInputError(`share image input ${path} does not exist`);
  try {
    return readFileSync(absolute, encoding);
  } catch (error) {
    throw new OgImageInputError(`cannot read share image input ${path}: ${error.message}`);
  }
}

export function loadOgImageInputs() {
  const params = loadIconParams();
  const tokens = readSiteTokens(readInputFile(OG_TOKENS_PATH, "utf8"));
  const font = readInputFile(OG_FONT_PATH);
  return Object.freeze({
    variant: params.variant,
    iconSvg: buildIconSvg(params, "flat"),
    fontBase64: font.toString("base64"),
    tokens,
  });
}

export function buildOgImageHtml(inputs) {
  const { iconSvg, fontBase64, tokens } = inputs;
  const iconPx = tokens.iconPx * OG_LOCKUP_SCALE;
  const gapPx = tokens.gapPx * OG_LOCKUP_SCALE;
  const fontPx = tokens.fontPx * OG_LOCKUP_SCALE;
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<style>
@font-face {
  font-family: "${OG_FONT_FAMILY}";
  src: url(data:font/woff2;base64,${fontBase64}) format("woff2");
  font-weight: 400 800;
}
html, body {
  margin: 0;
  width: ${OG_WIDTH}px;
  height: ${OG_HEIGHT}px;
  overflow: hidden;
  background: ${tokens.canvas};
}
body {
  display: flex;
  align-items: center;
  justify-content: center;
}
.mark {
  display: flex;
  align-items: center;
  gap: ${gapPx}px;
  color: ${tokens.fg};
  font-family: "${OG_FONT_FAMILY}";
  font-weight: ${OG_FONT_WEIGHT};
  font-size: ${fontPx}px;
  line-height: 1;
  font-synthesis: none;
  -webkit-font-smoothing: antialiased;
}
.mark svg {
  display: block;
  flex-shrink: 0;
  width: ${iconPx}px;
  height: ${iconPx}px;
}
</style>
</head>
<body><div class="mark">${iconSvg}<span>${OG_NAME}</span></div></body>
</html>
`;
}

function firstLine(message) {
  return String(message).split("\n")[0];
}

export async function renderOgImage(html, options = {}) {
  const chromiumPath = options.chromiumPath ?? null;
  let chromium;
  try {
    ({ chromium } = await import("playwright-core"));
  } catch (error) {
    throw new OgImageRenderError(`playwright-core did not load (${firstLine(error.message)}); run pnpm install`);
  }
  let browser;
  try {
    browser = await chromium.launch({ headless: true, ...(chromiumPath === null ? {} : { executablePath: chromiumPath }) });
  } catch (error) {
    throw new OgImageRenderError(
      `chromium did not launch (${firstLine(error.message)}); run pnpm exec playwright-core install chromium-headless-shell or pass --chromium <path>`,
    );
  }
  try {
    const page = await browser.newPage({ viewport: { width: OG_WIDTH, height: OG_HEIGHT }, deviceScaleFactor: 1 });
    await page.setContent(html, { waitUntil: "load" });
    const fontLoaded = await page.evaluate(
      async ({ family, weight, text }) => {
        await document.fonts.load(`${weight} 16px "${family}"`, text);
        await document.fonts.ready;
        return [...document.fonts].some((face) => face.family.replace(/["']/g, "") === family && face.status === "loaded");
      },
      { family: OG_FONT_FAMILY, weight: OG_FONT_WEIGHT, text: OG_NAME },
    );
    if (!fontLoaded) throw new OgImageRenderError(`${OG_FONT_FAMILY} did not load from ${OG_FONT_PATH}`);
    return await page.screenshot({ type: "png", animations: "disabled" });
  } catch (error) {
    if (error instanceof OgImageRenderError) throw error;
    throw new OgImageRenderError(`share image render failed: ${firstLine(error.message)}`);
  } finally {
    await browser.close();
  }
}

export function readPngSize(png) {
  if (png.length < 24 || !png.subarray(0, 8).equals(PNG_SIGNATURE) || png.toString("latin1", 12, 16) !== "IHDR") {
    throw new OgImageOutputError("share image is not a PNG with an IHDR header");
  }
  return Object.freeze({ width: png.readUInt32BE(16), height: png.readUInt32BE(20) });
}

export function writeOgImage(outPath, png) {
  const { width, height } = readPngSize(png);
  if (width !== OG_WIDTH || height !== OG_HEIGHT) {
    throw new OgImageOutputError(`share image rendered at ${width}x${height}; expected ${OG_WIDTH}x${OG_HEIGHT}`);
  }
  try {
    mkdirSync(dirname(outPath), { recursive: true });
    writeFileSync(outPath, png);
  } catch (error) {
    throw new OgImageOutputError(`cannot write share image ${outPath}: ${error.message}`);
  }
}
