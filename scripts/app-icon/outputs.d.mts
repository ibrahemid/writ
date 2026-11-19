import type { IconForm, IconParams } from "./render.mjs";

export interface AppIconFile {
  readonly path: string;
  readonly kind: "svg" | "png" | "ico" | "icns";
  readonly form: IconForm;
  readonly size?: number;
}

export declare const APP_ICON_FILES: readonly AppIconFile[];

export declare class IconOutputError extends Error {}

export declare function buildAppIconFiles(params: IconParams): Map<string, Buffer | string>;
export declare function writeAppIconFiles(outRoot: string, files: ReadonlyMap<string, Buffer | string>): void;
