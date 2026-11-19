// @vitest-environment node
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { PARAMS_PATH, ROOT, loadIconParams } from "../../app-icon/render.mjs";
import {
  OG_FONT_FAMILY,
  OG_FONT_PATH,
  OG_FONT_WEIGHT,
  OG_HEIGHT,
  OG_IMAGE_PATH,
  OG_NAME,
  OG_TOKENS_PATH,
  OG_WIDTH,
  OgImageInputError,
  OgImageOutputError,
  buildOgImageHtml,
  loadOgImageInputs,
  readPngSize,
  readSiteTokens,
  writeOgImage,
} from "../build.mjs";
import { OgImageArgsError, parseOgImageArgs } from "../cli.mjs";

const VALID_TOKENS = `:root {
  --jotra-space-3: 8px;
  --jotra-space-6: 24px;
  --jotra-bg-canvas: #ffffff;
  --jotra-fg: #1C1A17;
  --jotra-site-text-md: 17px;
}`;

describe("share image inputs", () => {
  it("finds the icon parameters, the display font and the site tokens", () => {
    for (const path of [PARAMS_PATH, resolve(ROOT, OG_FONT_PATH), resolve(ROOT, OG_TOKENS_PATH)]) {
      expect(existsSync(path), path).toBe(true);
    }
  });

  it("draws the shipped icon variant", () => {
    expect(loadOgImageInputs().variant).toBe(loadIconParams().shippedVariant);
  });

  it("reads the canvas colour and the nav lockup from the site tokens", () => {
    const tokens = readSiteTokens(readFileSync(resolve(ROOT, OG_TOKENS_PATH), "utf8"));
    expect(tokens.canvas).toMatch(/^#[0-9A-F]{6}$/);
    expect(tokens.fg).toMatch(/^#[0-9A-F]{6}$/);
    for (const px of [tokens.iconPx, tokens.gapPx, tokens.fontPx]) expect(px).toBeGreaterThan(0);
  });

  it("parses colours and lengths", () => {
    expect(readSiteTokens(VALID_TOKENS)).toEqual({ canvas: "#FFFFFF", fg: "#1C1A17", iconPx: 24, gapPx: 8, fontPx: 17 });
  });

  it("rejects a missing token", () => {
    expect(() => readSiteTokens(VALID_TOKENS.replace("--jotra-fg: #1C1A17;", ""))).toThrow(OgImageInputError);
  });

  it("rejects a token defined twice", () => {
    expect(() => readSiteTokens(`${VALID_TOKENS}\n:root {\n  --jotra-bg-canvas: #000000;\n}`)).toThrow(/defines --jotra-bg-canvas 2 times/);
  });

  it("rejects a colour that is not #RRGGBB and a length that is not px", () => {
    expect(() => readSiteTokens(VALID_TOKENS.replace("#ffffff", "white"))).toThrow(OgImageInputError);
    expect(() => readSiteTokens(VALID_TOKENS.replace("24px", "1.5rem"))).toThrow(OgImageInputError);
  });
});

describe("share image page", () => {
  const inputs = loadOgImageInputs();
  const html = buildOgImageHtml(inputs);

  it("holds the mark and the name only", () => {
    const open = "<body>";
    const body = html.slice(html.indexOf(open) + open.length, html.indexOf("</body>"));
    expect(body).toBe(`<div class="mark">${inputs.iconSvg}<span>${OG_NAME}</span></div>`);
  });

  it("sets the name in the display font on the canvas colour", () => {
    expect(html).toContain(`font-family: "${OG_FONT_FAMILY}";`);
    expect(html).toContain(`font-weight: ${OG_FONT_WEIGHT};`);
    expect(html).toContain(`background: ${inputs.tokens.canvas};`);
    expect(html).toContain(`color: ${inputs.tokens.fg};`);
  });
});

describe("share image output", () => {
  it(`is ${OG_WIDTH} by ${OG_HEIGHT}`, () => {
    expect(readPngSize(readFileSync(resolve(ROOT, OG_IMAGE_PATH)))).toEqual({ width: OG_WIDTH, height: OG_HEIGHT });
  });

  it("rejects a file that is not a PNG", () => {
    expect(() => readPngSize(Buffer.from("not a png file at all"))).toThrow(OgImageOutputError);
  });

  it("refuses to write a render at the wrong size", () => {
    const png = Buffer.from(readFileSync(resolve(ROOT, OG_IMAGE_PATH)));
    png.writeUInt32BE(OG_WIDTH * 2, 16);
    expect(() => writeOgImage(resolve(ROOT, "unused.png"), png)).toThrow(/rendered at 2400x630/);
  });
});

describe("share image arguments", () => {
  it("writes into the site with the bundled browser when no flag is given", () => {
    expect(parseOgImageArgs([])).toEqual({ outPath: resolve(ROOT, OG_IMAGE_PATH), chromiumPath: null });
  });

  it("rejects an output that is not a PNG file", () => {
    expect(() => parseOgImageArgs(["--out", "share.jpg"])).toThrow(OgImageArgsError);
  });

  it("rejects a browser path that does not exist", () => {
    expect(() => parseOgImageArgs(["--chromium", resolve(ROOT, "no-such-browser")])).toThrow(OgImageArgsError);
  });

  it("rejects unknown flags", () => {
    expect(() => parseOgImageArgs(["--variant", "j-dot-bold"])).toThrow(OgImageArgsError);
  });
});
