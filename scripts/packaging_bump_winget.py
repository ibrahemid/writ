#!/usr/bin/env python3
"""Bump the winget manifests to a new version.

Copies the highest existing version folder to `VERSION` if needed, then rewrites
version, installer URL, installer SHA256, release date, and release notes URL.
"""

from __future__ import annotations

import os
import pathlib
import re
import shutil
import sys

MANIFEST_ROOT = pathlib.Path("packaging/winget/manifests/i/ibrahemid/Writ")


def get_env(name: str) -> str:
    value = os.environ.get(name, "")
    if not value:
        print(f"Missing environment variable: {name}", file=sys.stderr)
        sys.exit(1)
    return value


def version_key(name: str) -> tuple[int, ...]:
    parts = re.findall(r"\d+", name)
    return tuple(int(p) for p in parts) if parts else (0,)


def main() -> None:
    version = get_env("VERSION")
    sha_msi = get_env("SHA_MSI")
    release_date = get_env("RELEASE_DATE")

    if not MANIFEST_ROOT.is_dir():
        print(f"winget manifest root missing: {MANIFEST_ROOT}", file=sys.stderr)
        sys.exit(1)

    existing = sorted(
        (p for p in MANIFEST_ROOT.iterdir() if p.is_dir()),
        key=lambda p: version_key(p.name),
    )
    if not existing:
        print(f"No existing version folders in {MANIFEST_ROOT}", file=sys.stderr)
        sys.exit(1)

    source_dir = existing[-1]
    target_dir = MANIFEST_ROOT / version

    if source_dir != target_dir:
        if target_dir.exists():
            shutil.rmtree(target_dir)
        shutil.copytree(source_dir, target_dir)
        print(f"Copied {source_dir} -> {target_dir}")

    installer_url = (
        f"https://github.com/ibrahemid/writ/releases/download/v{version}/"
        f"Writ_{version}_x64_en-US.msi"
    )
    release_notes_url = f"https://github.com/ibrahemid/writ/releases/tag/v{version}"

    for path in sorted(target_dir.glob("*.yaml")):
        text = path.read_text()
        text = re.sub(
            r"^PackageVersion: .*$",
            f"PackageVersion: {version}",
            text,
            flags=re.MULTILINE,
        )
        if path.name.endswith("installer.yaml"):
            text = re.sub(
                r"InstallerUrl: https://github\.com/ibrahemid/writ/releases/download/v[^\s]+",
                f"InstallerUrl: {installer_url}",
                text,
            )
            text = re.sub(
                r"InstallerSha256: .*",
                f"InstallerSha256: {sha_msi}",
                text,
            )
            text = re.sub(
                r"^ReleaseDate: .*$",
                f"ReleaseDate: {release_date}",
                text,
                flags=re.MULTILINE,
            )
        if path.name.endswith("locale.en-US.yaml"):
            text = re.sub(
                r"ReleaseNotesUrl: https://github\.com/ibrahemid/writ/releases/tag/v.*",
                f"ReleaseNotesUrl: {release_notes_url}",
                text,
            )
        path.write_text(text)
        print(f"Rewrote {path}")


if __name__ == "__main__":
    main()
