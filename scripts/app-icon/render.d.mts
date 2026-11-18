export type IconForm = "macos" | "flat";
export type IconPoint = readonly [number, number];
export type PrimitiveType = "polyline" | "arc" | "dot";

export interface IconConfig {
  readonly canvas: number;
  readonly variant: string;
  readonly exponent: number;
  readonly bodies: Readonly<Record<IconForm, number>>;
  readonly tileHex: string;
  readonly tileRgb: readonly [number, number, number];
  readonly glyphHex: string;
  readonly glyphRgb: readonly [number, number, number];
  readonly frame: number;
}

export interface PolylinePrimitive {
  readonly id: string;
  readonly type: "polyline";
  readonly points: readonly IconPoint[];
}

export interface ArcPrimitive {
  readonly id: string;
  readonly type: "arc";
  readonly center: IconPoint;
  readonly radius: number;
  readonly startDegrees: number;
  readonly sweepDegrees: number;
}

export interface DotPrimitive {
  readonly id: string;
  readonly type: "dot";
  readonly center: IconPoint;
  readonly diameterInStrokes: number;
}

export type GlyphPrimitive = PolylinePrimitive | ArcPrimitive | DotPrimitive;

export interface OpticalEntry {
  readonly target: string;
  readonly sizes: readonly number[];
  readonly forms: readonly IconForm[];
  readonly translatePx: IconPoint;
  readonly diameterPx: number | null;
}

export interface IconVariant {
  readonly name: string;
  readonly strokeWidth: number;
  readonly strokeWidthForm: IconForm;
  readonly smallSizeStrokes: readonly (readonly [number, number])[];
  readonly proportionalStrokeFrom: number;
  readonly primitives: readonly GlyphPrimitive[];
  readonly optical: readonly OpticalEntry[];
}

export interface IconParams {
  readonly canvas: number;
  readonly exponent: number;
  readonly bodies: Readonly<Record<IconForm, number>>;
  readonly tileHex: string;
  readonly tileRgb: readonly [number, number, number];
  readonly glyphHex: string;
  readonly glyphRgb: readonly [number, number, number];
  readonly frame: number;
  readonly shippedVariant: string;
  readonly variant: string;
  readonly strokeWidth: number;
  readonly strokeWidthForm: IconForm;
  readonly smallSizeStrokes: readonly (readonly [number, number])[];
  readonly proportionalStrokeFrom: number;
  readonly primitives: readonly GlyphPrimitive[];
  readonly optical: readonly OpticalEntry[];
}

export interface PolylineLayout {
  readonly id: string;
  readonly type: "polyline";
  readonly points: readonly IconPoint[];
  readonly reach: number;
}

export interface ArcLayout {
  readonly id: string;
  readonly type: "arc";
  readonly center: IconPoint;
  readonly radius: number;
  readonly startDegrees: number;
  readonly sweepDegrees: number;
  readonly reach: number;
}

export interface DotLayout {
  readonly id: string;
  readonly type: "dot";
  readonly center: IconPoint;
  readonly reach: number;
}

export type PrimitiveLayout = PolylineLayout | ArcLayout | DotLayout;

export interface IconLayout {
  readonly size: number;
  readonly centre: number;
  readonly halfBody: number;
  readonly exponent: number;
  readonly strokeRadius: number;
  readonly primitives: readonly PrimitiveLayout[];
}

export interface RasterImage {
  readonly width: number;
  readonly height: number;
  readonly rgba: Uint8Array;
}

export interface LoadIconParamsOptions {
  readonly variant?: string;
  readonly configPath?: string;
  readonly variantsDir?: string;
}

export declare const ROOT: string;
export declare const PARAMS_PATH: string;
export declare const VARIANTS_DIR: string;
export declare const ICON_FORMS: readonly IconForm[];
export declare const PRIMITIVE_TYPES: readonly PrimitiveType[];
export declare const GLYPH_TARGET: "glyph";

export declare class IconParamsError extends Error {}
export declare class IconRenderError extends Error {}

export declare function parseIconConfig(raw: unknown): IconConfig;
export declare function parseIconVariant(raw: unknown, name: string): IconVariant;
export declare function parseIconParams(configRaw: unknown, variantRaw: unknown, variantName?: string): IconParams;
export declare function listIconVariants(variantsDir?: string): string[];
export declare function loadIconParams(options?: LoadIconParamsOptions): IconParams;
export declare function computeBodyPx(params: IconParams, form: IconForm, size: number): number;
export declare function computeStrokePx(params: IconParams, form: IconForm, size: number): number;
export declare function layoutIcon(params: IconParams, form: IconForm, size: number): IconLayout;
export declare function renderIcon(params: IconParams, form: IconForm, size: number): RasterImage;
export declare function buildIconSvg(params: IconParams, form: IconForm): string;
