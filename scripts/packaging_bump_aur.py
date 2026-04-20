#!/usr/bin/env python3
"""Bump the AUR `writ-bin` PKGBUILD and regenerate `.SRCINFO`."""

from __future__ import annotations

import os
import pathlib
import re
import sys

PKGBUILD_PATH = pathlib.Path("packaging/aur/writ-bin/PKGBUILD")
SRCINFO_PATH = pathlib.Path("packaging/aur/writ-bin/.SRCINFO")


def get_env(name: str) -> str:
    value = os.environ.get(name, "")
    if not value:
        print(f"Missing environment variable: {name}", file=sys.stderr)
        sys.exit(1)
    return value


def bump_pkgbuild(version: str, sha_appimage: str) -> None:
    if not PKGBUILD_PATH.is_file():
        print(f"PKGBUILD missing: {PKGBUILD_PATH}", file=sys.stderr)
        sys.exit(1)
    text = PKGBUILD_PATH.read_text()

    text, n_ver = re.subn(
        r"^pkgver=.*$",
        f"pkgver={version}",
        text,
        flags=re.MULTILINE,
        count=1,
    )
    text, n_rel = re.subn(
        r"^pkgrel=.*$",
        "pkgrel=1",
        text,
        flags=re.MULTILINE,
        count=1,
    )
    if n_ver != 1 or n_rel != 1:
        print("Failed to rewrite pkgver/pkgrel", file=sys.stderr)
        sys.exit(1)

    new_sums = f"sha256sums=('{sha_appimage}'\n            'SKIP')"
    text, n_sums = re.subn(
        r"sha256sums=\('[^']+'\s*'SKIP'\)",
        new_sums,
        text,
        count=1,
    )
    if n_sums != 1:
        print("Failed to rewrite sha256sums array", file=sys.stderr)
        sys.exit(1)

    PKGBUILD_PATH.write_text(text)
    print(f"Bumped {PKGBUILD_PATH} to {version}")


def write_srcinfo(version: str, sha_appimage: str) -> None:
    content = (
        "pkgbase = writ-bin\n"
        "\tpkgdesc = Lightweight, always-ready text editor for developers\n"
        f"\tpkgver = {version}\n"
        "\tpkgrel = 1\n"
        "\turl = https://github.com/ibrahemid/writ\n"
        "\tarch = x86_64\n"
        "\tlicense = MIT\n"
        "\tdepends = glibc\n"
        "\tdepends = fuse2\n"
        "\tdepends = gtk3\n"
        "\tdepends = webkit2gtk-4.1\n"
        "\toptdepends = appimagelauncher: desktop integration for AppImages\n"
        "\tprovides = writ\n"
        "\tconflicts = writ\n"
        "\toptions = !strip\n"
        f"\tsource = writ-{version}.AppImage::"
        f"https://github.com/ibrahemid/writ/releases/download/v{version}/"
        f"writ_{version}_amd64.AppImage\n"
        f"\tsource = LICENSE-{version}::"
        f"https://github.com/ibrahemid/writ/raw/v{version}/LICENSE\n"
        f"\tnoextract = writ-{version}.AppImage\n"
        f"\tsha256sums = {sha_appimage}\n"
        "\tsha256sums = SKIP\n"
        "\n"
        "pkgname = writ-bin\n"
    )
    SRCINFO_PATH.write_text(content)
    print(f"Wrote {SRCINFO_PATH}")


def main() -> None:
    version = get_env("VERSION")
    sha_appimage = get_env("SHA_APPIMAGE")
    bump_pkgbuild(version, sha_appimage)
    write_srcinfo(version, sha_appimage)


if __name__ == "__main__":
    main()
