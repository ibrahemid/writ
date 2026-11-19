import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { ICNS_LAYOUT, ICO_SIZES, encodeIcns, encodeIco, encodePng } from "./containers.mjs";
import { buildIconSvg, renderIcon } from "./render.mjs";

export const APP_ICON_FILES = Object.freeze(
  [
    { path: "design/icon/app-icon.svg", kind: "svg", form: "macos" },
    { path: "src-tauri/icons/icon.icns", kind: "icns", form: "macos" },
    { path: "src-tauri/icons/icon.ico", kind: "ico", form: "flat" },
    { path: "src-tauri/icons/32x32.png", kind: "png", form: "flat", size: 32 },
    { path: "src-tauri/icons/128x128.png", kind: "png", form: "flat", size: 128 },
    { path: "src-tauri/icons/128x128@2x.png", kind: "png", form: "flat", size: 256 },
    { path: "site/public/brand/icon-32.png", kind: "png", form: "flat", size: 32 },
    { path: "site/public/brand/icon-128.png", kind: "png", form: "flat", size: 128 },
    { path: "site/public/brand/icon-256.png", kind: "png", form: "flat", size: 256 },
  ].map((file) => Object.freeze(file)),
);

export class IconOutputError extends Error {
  constructor(message) {
    super(message);
    this.name = "IconOutputError";
  }
}

export function buildAppIconFiles(params) {
  const renders = new Map();
  const renderCached = (form, size) => {
    const key = `${form}:${size}`;
    if (!renders.has(key)) renders.set(key, renderIcon(params, form, size));
    return renders.get(key);
  };
  const files = new Map();
  for (const file of APP_ICON_FILES) {
    switch (file.kind) {
      case "svg":
        files.set(file.path, buildIconSvg(params, file.form));
        break;
      case "png":
        files.set(file.path, encodePng(renderCached(file.form, file.size)));
        break;
      case "ico":
        files.set(file.path, encodeIco(ICO_SIZES.map((size) => renderCached(file.form, size))));
        break;
      case "icns":
        files.set(file.path, encodeIcns(new Map(ICNS_LAYOUT.map(({ size }) => [size, renderCached(file.form, size)]))));
        break;
      default:
        throw new IconOutputError(`app icon output ${file.path} has unknown kind ${JSON.stringify(file.kind)}`);
    }
  }
  return files;
}

export function writeAppIconFiles(outRoot, files) {
  for (const [path, contents] of files) {
    const target = resolve(outRoot, path);
    try {
      mkdirSync(dirname(target), { recursive: true });
      writeFileSync(target, contents);
    } catch (error) {
      throw new IconOutputError(`cannot write app icon output ${target}: ${error.message}`);
    }
  }
}
