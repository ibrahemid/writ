#!/usr/bin/env python3
"""Bump the AUR `writ-bin` PKGBUILD and regenerate `.SRCINFO`."""

from __future__ import annotations

import os
import pathlib
import re
import shlex
import sys

PKGBUILD_PATH = pathlib.Path("packaging/aur/writ-bin/PKGBUILD")
SRCINFO_PATH = pathlib.Path("packaging/aur/writ-bin/.SRCINFO")
PACKAGE_DESCRIPTION = "Jotra, a light text editor for any text file, with full-text search and inline Markdown"
SCALAR_ASSIGNMENT = re.compile(r"^([A-Za-z_][A-Za-z0-9_]*)=([^(\n][^\n]*)$", re.MULTILINE)
VARIABLE_REFERENCE = re.compile(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}")


def get_env(name: str) -> str:
    value = os.environ.get(name, "")
    if not value:
        print(f"Missing environment variable: {name}", file=sys.stderr)
        sys.exit(1)
    return value


class PkgbuildFormatError(ValueError):
    """The PKGBUILD does not contain every field the release must replace."""


def rewrite_pkgbuild(text: str, version: str, sha_appimage: str, sha_license: str) -> str:
    text, n_ver = re.subn(r"^pkgver=.*$", f"pkgver={version}", text, flags=re.MULTILINE, count=1)
    text, n_rel = re.subn(r"^pkgrel=.*$", "pkgrel=1", text, flags=re.MULTILINE, count=1)
    text, n_desc = re.subn(
        r"^pkgdesc=.*$",
        f'pkgdesc="{PACKAGE_DESCRIPTION}"',
        text,
        flags=re.MULTILINE,
        count=1,
    )
    new_sums = f"sha256sums=('{sha_appimage}'\n            'SKIP'\n            '{sha_license}')"
    text, n_sums = re.subn(r"sha256sums=\([^)]*\)", new_sums, text, count=1)
    missing = [
        name
        for name, count in (("pkgver", n_ver), ("pkgrel", n_rel), ("pkgdesc", n_desc), ("sha256sums", n_sums))
        if count != 1
    ]
    if missing:
        raise PkgbuildFormatError(f"PKGBUILD is missing {', '.join(missing)}")
    return text


def bump_pkgbuild(version: str, sha_appimage: str, sha_license: str) -> None:
    if not PKGBUILD_PATH.is_file():
        print(f"PKGBUILD missing: {PKGBUILD_PATH}", file=sys.stderr)
        sys.exit(1)
    try:
        text = rewrite_pkgbuild(PKGBUILD_PATH.read_text(), version, sha_appimage, sha_license)
    except PkgbuildFormatError as error:
        print(error, file=sys.stderr)
        sys.exit(1)
    PKGBUILD_PATH.write_text(text)
    print(f"Bumped {PKGBUILD_PATH} to {version}")


def read_pkgbuild_scalars(text: str) -> dict[str, str]:
    scalars: dict[str, str] = {}
    for name, value in SCALAR_ASSIGNMENT.findall(text):
        try:
            words = shlex.split(value, comments=True)
        except ValueError as error:
            raise PkgbuildFormatError(f"PKGBUILD assigns {name} an unbalanced value: {error}") from error
        if len(words) == 1:
            scalars[name] = words[0]
    return scalars


def read_pkgbuild_array(text: str, name: str) -> list[str]:
    match = re.search(rf"^{re.escape(name)}=\(([^)]*)\)", text, flags=re.MULTILINE)
    if match is None:
        raise PkgbuildFormatError(f"PKGBUILD is missing {name}")
    scalars = read_pkgbuild_scalars(text)

    def expand(reference: re.Match[str]) -> str:
        variable = reference.group(1)
        if variable not in scalars:
            raise PkgbuildFormatError(f"PKGBUILD {name} names ${{{variable}}}, which it does not assign one word")
        return scalars[variable]

    return [VARIABLE_REFERENCE.sub(expand, word) for word in shlex.split(match.group(1), comments=True)]


def srcinfo_text(pkgbuild: str, version: str, sha_appimage: str, sha_license: str) -> str:
    provides = "".join(f"\tprovides = {name}\n" for name in read_pkgbuild_array(pkgbuild, "provides"))
    conflicts = "".join(f"\tconflicts = {name}\n" for name in read_pkgbuild_array(pkgbuild, "conflicts"))
    return (
        "pkgbase = writ-bin\n"
        f"\tpkgdesc = {PACKAGE_DESCRIPTION}\n"
        f"\tpkgver = {version}\n"
        "\tpkgrel = 1\n"
        "\turl = https://github.com/ibrahemid/jotra\n"
        "\tarch = x86_64\n"
        "\tlicense = MIT\n"
        "\tmakedepends = minisign\n"
        "\tdepends = glibc\n"
        "\tdepends = fuse2\n"
        "\tdepends = gtk3\n"
        "\tdepends = webkit2gtk-4.1\n"
        "\tdepends = alsa-lib\n"
        "\toptdepends = appimagelauncher: desktop integration for AppImages\n"
        f"{provides}"
        f"{conflicts}"
        "\toptions = !strip\n"
        f"\tsource = jotra-{version}.AppImage::"
        f"https://github.com/ibrahemid/jotra/releases/download/v{version}/"
        f"Jotra_{version}_amd64.AppImage\n"
        f"\tsource = jotra-{version}.AppImage.minisign::"
        f"https://github.com/ibrahemid/jotra/releases/download/v{version}/"
        f"Jotra_{version}_amd64.AppImage.sig\n"
        f"\tsource = LICENSE-{version}::"
        f"https://github.com/ibrahemid/jotra/raw/v{version}/LICENSE\n"
        f"\tnoextract = jotra-{version}.AppImage\n"
        f"\tsha256sums = {sha_appimage}\n"
        "\tsha256sums = SKIP\n"
        f"\tsha256sums = {sha_license}\n"
        "\n"
        "pkgname = writ-bin\n"
    )


def write_srcinfo(version: str, sha_appimage: str, sha_license: str) -> None:
    try:
        text = srcinfo_text(PKGBUILD_PATH.read_text(), version, sha_appimage, sha_license)
    except PkgbuildFormatError as error:
        print(error, file=sys.stderr)
        sys.exit(1)
    SRCINFO_PATH.write_text(text)
    print(f"Wrote {SRCINFO_PATH}")


def main() -> None:
    version = get_env("VERSION")
    sha_appimage = get_env("SHA_APPIMAGE")
    sha_license = get_env("SHA_LICENSE")
    bump_pkgbuild(version, sha_appimage, sha_license)
    write_srcinfo(version, sha_appimage, sha_license)


if __name__ == "__main__":
    main()
