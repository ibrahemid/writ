import type { RasterImage } from "./render.mjs";

export interface IcnsLayoutEntry {
  readonly type: string;
  readonly size: number;
  readonly encoding: "png" | "rle";
  readonly mask?: string;
}

export interface IcoEntry {
  readonly width: number;
  readonly height: number;
  readonly colorCount: number;
  readonly planes: number;
  readonly bitCount: number;
  readonly size: number;
  readonly offset: number;
  readonly data: Buffer;
}

export interface IcnsElement {
  readonly type: string;
  readonly data: Buffer;
}

export declare const ICO_SIZES: readonly number[];
export declare const ICNS_LAYOUT: readonly IcnsLayoutEntry[];

export declare class IconFormatError extends Error {}

export declare function encodePng(image: RasterImage): Buffer;
export declare function decodePng(bytes: Uint8Array): RasterImage;
export declare function encodeIco(images: readonly RasterImage[]): Buffer;
export declare function parseIco(bytes: Uint8Array): IcoEntry[];
export declare function encodeIcnsRle(image: RasterImage): Buffer;
export declare function decodeIcnsRle(bytes: Uint8Array, pixelCount: number): Uint8Array;
export declare function encodeIcns(imagesBySize: ReadonlyMap<number, RasterImage>): Buffer;
export declare function parseIcns(bytes: Uint8Array): IcnsElement[];
export declare function decodeIcnsImage(elements: readonly IcnsElement[], type: string): RasterImage;
