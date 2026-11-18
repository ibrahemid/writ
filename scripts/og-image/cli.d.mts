export interface OgImageArgs {
  readonly outPath: string;
  readonly chromiumPath: string | null;
}

export interface OgImageBuildResult {
  readonly variant: string;
  readonly outPath: string;
  readonly width: number;
  readonly height: number;
}

export declare class OgImageArgsError extends Error {}

export declare const OG_IMAGE_ERRORS: readonly (new (message: string) => Error)[];

export declare function parseOgImageArgs(argv: readonly string[]): OgImageArgs;
export declare function runOgImageBuild(argv: readonly string[]): Promise<OgImageBuildResult>;
