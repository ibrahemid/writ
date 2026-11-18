import { realpathSync } from "node:fs";
import { resolve } from "node:path";
import { parseArgs } from "node:util";
import { IconFormatError } from "./containers.mjs";
import { IconOutputError, buildAppIconFiles, writeAppIconFiles } from "./outputs.mjs";
import { IconParamsError, IconRenderError, ROOT, loadIconParams } from "./render.mjs";

const USAGE = "usage: node scripts/build-app-icon.mjs [--out <dir>] [--variant <name> --out <dir>]";

export class IconBuildArgsError extends Error {
  constructor(message) {
    super(message);
    this.name = "IconBuildArgsError";
  }
}

export const ICON_BUILD_ERRORS = Object.freeze([
  IconBuildArgsError,
  IconParamsError,
  IconRenderError,
  IconOutputError,
  IconFormatError,
]);

function resolveRealPath(path) {
  try {
    return realpathSync(path);
  } catch {
    return resolve(path);
  }
}

export function parseBuildArgs(argv) {
  let values;
  try {
    ({ values } = parseArgs({
      args: [...argv],
      options: { variant: { type: "string" }, out: { type: "string" } },
      strict: true,
      allowPositionals: false,
    }));
  } catch (error) {
    throw new IconBuildArgsError(`${error.message}; ${USAGE}`);
  }
  if (values.out !== undefined && values.out.trim() === "") throw new IconBuildArgsError(`--out needs a directory; ${USAGE}`);
  if (values.variant !== undefined && values.out === undefined) {
    throw new IconBuildArgsError(
      `--variant ${values.variant} needs --out <dir>; the tracked icon files come only from the variant named in design/icon/app-icon.json`,
    );
  }
  return Object.freeze({
    variant: values.variant ?? null,
    outRoot: values.out === undefined ? ROOT : resolve(values.out),
  });
}

export function isRepositoryRoot(path) {
  return resolveRealPath(path) === resolveRealPath(ROOT);
}

export function runAppIconBuild(argv) {
  const { variant, outRoot } = parseBuildArgs(argv);
  const params = loadIconParams(variant === null ? {} : { variant });
  if (params.variant !== params.shippedVariant && isRepositoryRoot(outRoot)) {
    throw new IconBuildArgsError(
      `--out ${outRoot} is the repository and ${params.variant} is not the shipped variant (${params.shippedVariant}); set "variant" in design/icon/app-icon.json to ship it`,
    );
  }
  const files = buildAppIconFiles(params);
  writeAppIconFiles(outRoot, files);
  return Object.freeze({ variant: params.variant, outRoot, paths: Object.freeze([...files.keys()]) });
}
