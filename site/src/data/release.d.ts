declare module '*/release.json' {
  interface ReleaseAssets {
    'mac-universal': string;
    'win-x64': string;
    'linux-appimage': string;
    'linux-deb': string;
  }
  interface Release {
    version: string;
    tag: string;
    publishedAt: string;
    assets: ReleaseAssets;
  }
  const value: Release;
  export default value;
}
