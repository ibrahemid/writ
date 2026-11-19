import { existsSync } from "node:fs";
import { extname, resolve } from "node:path";
import { parseArgs } from "node:util";
import { IconParamsError, IconRenderError, ROOT } from "../app-icon/render.mjs";
import {
  OG_HEIGHT,
  OG_IMAGE_PATH,
  OG_WIDTH,
  OgImageInputError,
  OgImageOutputError,
  OgImageRenderError,
  buildOgImageHtml,
  loadOgImageInputs,
  renderOgImage,
  writeOgImage,
} from "./build.mjs";

const USAGE = "usage: node scripts/build-og-image.mjs [--out <file.png>] [--chromium <path>]";

export class OgImageArgsError extends Error {
  constructor(message) {
    super(message);
    this.name = "OgImageArgsError";
  }
}

export const OG_IMAGE_ERRORS = Object.freeze([
  OgImageArgsError,
  OgImageInputError,
  OgImageRenderError,
  OgImageOutputError,
  IconParamsError,
  IconRenderError,
]);

export function parseOgImageArgs(argv) {
  let values;
  try {
    ({ values } = parseArgs({
      args: [...argv],
      options: { out: { type: "string" }, chromium: { type: "string" } },
      strict: true,
      allowPositionals: false,
    }));
  } catch (error) {
    throw new OgImageArgsError(`${error.message}; ${USAGE}`);
  }
  if (values.out !== undefined && extname(values.out).toLowerCase() !== ".png") {
    throw new OgImageArgsError(`--out needs a .png file; ${USAGE}`);
  }
  if (values.chromium !== undefined && !existsSync(values.chromium)) {
    throw new OgImageArgsError(`--chromium ${values.chromium} does not exist; ${USAGE}`);
  }
  return Object.freeze({
    outPath: values.out === undefined ? resolve(ROOT, OG_IMAGE_PATH) : resolve(values.out),
    chromiumPath: values.chromium === undefined ? null : resolve(values.chromium),
  });
}

export async function runOgImageBuild(argv) {
  const { outPath, chromiumPath } = parseOgImageArgs(argv);
  const inputs = loadOgImageInputs();
  const png = await renderOgImage(buildOgImageHtml(inputs), { chromiumPath });
  writeOgImage(outPath, png);
  return Object.freeze({ variant: inputs.variant, outPath, width: OG_WIDTH, height: OG_HEIGHT });
}
