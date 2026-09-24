#!/usr/bin/env python3
"""Bump the AUR `writ-bin` PKGBUILD and regenerate `.SRCINFO`."""

from __future__ import annotations

import os
import pathlib
import re
import sys

PKGBUILD_PATH = pathlib.Path("packaging/aur/writ-bin/PKGBUILD")
SRCINFO_PATH = pathlib.Path("packaging/aur/writ-bin/.SRCINFO")
PACKAGE_DESCRIPTION = "Light text editor for any text file, with full-text search and inline Markdown"


def get_env(name: str) -> str:
    value = os.environ.get(name, "")
    if not value:
        print(f"Missing environment variable: {name}", file=sys.stderr)
        sys.exit(1)
    return value


class PkgbuildFormatError(ValueError):
    """The PKGBUILD does not contain every field the release must replace."""


def rewrite_pkgbuild(text: str, version: str, sha_appimage: str) -> str:
    text, n_ver = re.subn(r"^pkgver=.*$", f"pkgver={version}", text, flags=re.MULTILINE, count=1)
    text, n_rel = re.subn(r"^pkgrel=.*$", "pkgrel=1", text, flags=re.MULTILINE, count=1)
    text, n_desc = re.subn(
        r"^pkgdesc=.*$",
        f'pkgdesc="{PACKAGE_DESCRIPTION}"',
        text,
        flags=re.MULTILINE,
        count=1,
    )
    new_sums = f"sha256sums=('{sha_appimage}'\n            'SKIP')"
    text, n_sums = re.subn(r"sha256sums=\('[^']+'\s*'SKIP'\)", new_sums, text, count=1)
    missing = [
        name
        for name, count in (("pkgver", n_ver), ("pkgrel", n_rel), ("pkgdesc", n_desc), ("sha256sums", n_sums))
        if count != 1
    ]
    if missing:
        raise PkgbuildFormatError(f"PKGBUILD is missing {', '.join(missing)}")
    return text


def bump_pkgbuild(version: str, sha_appimage: str) -> None:
    if not PKGBUILD_PATH.is_file():
        print(f"PKGBUILD missing: {PKGBUILD_PATH}", file=sys.stderr)
        sys.exit(1)
    try:
        text = rewrite_pkgbuild(PKGBUILD_PATH.read_text(), version, sha_appimage)
    except PkgbuildFormatError as error:
        print(error, file=sys.stderr)
        sys.exit(1)
    PKGBUILD_PATH.write_text(text)
    print(f"Bumped {PKGBUILD_PATH} to {version}")


def write_srcinfo(version: str, sha_appimage: str) -> None:
    content = (
        "pkgbase = writ-bin\n"
        f"\tpkgdesc = {PACKAGE_DESCRIPTION}\n"
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
        f"Writ_{version}_amd64.AppImage\n"
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
