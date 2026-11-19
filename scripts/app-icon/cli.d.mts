export interface BuildArgs {
  readonly variant: string | null;
  readonly outRoot: string;
}

export interface AppIconBuildResult {
  readonly variant: string;
  readonly outRoot: string;
  readonly paths: readonly string[];
}

export declare class IconBuildArgsError extends Error {}

export declare const ICON_BUILD_ERRORS: readonly (new (message: string) => Error)[];

export declare function parseBuildArgs(argv: readonly string[]): BuildArgs;
export declare function isRepositoryRoot(path: string): boolean;
export declare function runAppIconBuild(argv: readonly string[]): AppIconBuildResult;
