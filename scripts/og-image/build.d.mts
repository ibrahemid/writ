export interface OgSiteTokens {
  readonly canvas: string;
  readonly fg: string;
  readonly iconPx: number;
  readonly gapPx: number;
  readonly fontPx: number;
}

export interface OgImageInputs {
  readonly variant: string;
  readonly iconSvg: string;
  readonly fontBase64: string;
  readonly tokens: OgSiteTokens;
}

export interface RenderOgImageOptions {
  readonly chromiumPath?: string | null;
}

export interface PngSize {
  readonly width: number;
  readonly height: number;
}

export declare const OG_IMAGE_PATH: string;
export declare const OG_FONT_PATH: string;
export declare const OG_TOKENS_PATH: string;
export declare const OG_WIDTH: number;
export declare const OG_HEIGHT: number;
export declare const OG_NAME: string;
export declare const OG_FONT_FAMILY: string;
export declare const OG_FONT_WEIGHT: number;
export declare const OG_LOCKUP_SCALE: number;
export declare const OG_SITE_TOKENS: Readonly<Record<keyof OgSiteTokens, string>>;

export declare class OgImageInputError extends Error {}
export declare class OgImageRenderError extends Error {}
export declare class OgImageOutputError extends Error {}

export declare function readSiteTokens(css: string): OgSiteTokens;
export declare function loadOgImageInputs(): OgImageInputs;
export declare function buildOgImageHtml(inputs: OgImageInputs): string;
export declare function renderOgImage(html: string, options?: RenderOgImageOptions): Promise<Buffer>;
export declare function readPngSize(png: Buffer): PngSize;
export declare function writeOgImage(outPath: string, png: Buffer): void;
