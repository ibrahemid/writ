declare module '*/release.json' {
  interface ReleaseMac {
    dmg: string;
    pkg: string;
    sha256_dmg: string;
    sha256_pkg: string;
  }
  interface ReleaseWin {
    msi: string;
    sha256_msi: string;
  }
  interface ReleaseLinux {
    deb: string;
    appImage: string;
    sha256_deb: string;
    sha256_appImage: string;
  }
  interface ReleasePlatforms {
    mac: ReleaseMac;
    win: ReleaseWin;
    linux: ReleaseLinux;
  }
  interface Release {
    version: string;
    tag: string;
    publishedAt: string;
    published: boolean;
    notarized: boolean;
    platforms: ReleasePlatforms;
  }
  const value: Release;
  export default value;
}
