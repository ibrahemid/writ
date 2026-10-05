#!/usr/bin/env python3
"""Bump the winget manifests to a new version.

Environment:
  VERSION       release version without the leading v
  SHA_MSI       SHA256 of the released x64 MSI
  RELEASE_DATE  YYYY-MM-DD
  MSI_PATH      the released x64 MSI; its ProductCode goes into the installer
                manifest, read with `msiinfo export <msi> Property` (msitools)
"""

from __future__ import annotations

import os
import pathlib
import re
import shutil
import subprocess
import sys

MANIFEST_ROOT = pathlib.Path("packaging/winget/manifests/i/ibrahemid/Writ")
SHORT_DESCRIPTION = "Light text editor for any text file, with full-text search and inline Markdown"
DESCRIPTION = (
    "Writ opens any text file and searches every file in its folder. A Markdown file renders "
    "inline as you type, with the markup shown on the line you are editing. Chat, rewriting, "
    "connected programs, connections, the graph and tags are apps, each switched on in Settings. "
    "Writ needs no account and sends no telemetry."
)
TAGS = ("text-editor", "editor", "markdown", "plain-text", "search", "tauri")


PRODUCT_CODE_PATTERN = re.compile(
    r"\{[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}\}"
)


class ManifestFormatError(ValueError):
    """A source manifest does not contain every field the release must replace."""


class MsiReadError(RuntimeError):
    """The released MSI's ProductCode could not be read."""


def get_env(name: str) -> str:
    value = os.environ.get(name, "")
    if not value:
        print(f"Missing environment variable: {name}", file=sys.stderr)
        sys.exit(1)
    return value


def version_key(name: str) -> tuple[int, ...]:
    parts = re.findall(r"\d+", name)
    return tuple(int(p) for p in parts) if parts else (0,)


def rewrite_locale(text: str, release_notes_url: str) -> str:
    text, short_count = re.subn(
        r"^ShortDescription: .*$",
        f"ShortDescription: {SHORT_DESCRIPTION}",
        text,
        count=1,
        flags=re.MULTILINE,
    )
    text, description_count = re.subn(
        r"^Description: \|-\n(?:  .*\n)+(?=Moniker:)",
        "Description: |-\n  " + DESCRIPTION + "\n",
        text,
        count=1,
        flags=re.MULTILINE,
    )
    tags = "Tags:\n" + "".join(f"  - {tag}\n" for tag in TAGS)
    text, tags_count = re.subn(
        r"^Tags:\n(?:  - .*\n)+(?=ReleaseNotesUrl:)",
        tags,
        text,
        count=1,
        flags=re.MULTILINE,
    )
    text, notes_count = re.subn(
        r"^ReleaseNotesUrl: https://github\.com/ibrahemid/writ/releases/tag/v.*$",
        f"ReleaseNotesUrl: {release_notes_url}",
        text,
        count=1,
        flags=re.MULTILINE,
    )
    if (short_count, description_count, tags_count, notes_count) != (1, 1, 1, 1):
        raise ManifestFormatError("winget locale is missing a required description field")
    return text


def parse_product_code(property_table: str) -> str:
    codes = [
        value
        for name, _, value in (line.partition("\t") for line in property_table.splitlines())
        if name == "ProductCode"
    ]
    if len(codes) != 1:
        raise MsiReadError(f"expected one ProductCode row in the MSI Property table, found {len(codes)}")
    code = codes[0].strip()
    if not PRODUCT_CODE_PATTERN.fullmatch(code):
        raise MsiReadError(f"MSI ProductCode is not a braced GUID: {code!r}")
    return code


def read_product_code(msi_path: pathlib.Path) -> str:
    if not msi_path.is_file():
        raise MsiReadError(f"released MSI not found: {msi_path}")
    try:
        result = subprocess.run(
            ["msiinfo", "export", str(msi_path), "Property"],
            capture_output=True,
            text=True,
        )
    except FileNotFoundError as error:
        raise MsiReadError("msiinfo not found; install msitools to read the MSI ProductCode") from error
    if result.returncode != 0:
        raise MsiReadError(
            f"msiinfo export {msi_path} Property exited {result.returncode}: {result.stderr.strip()}"
        )
    return parse_product_code(result.stdout)


def rewrite_installer(
    text: str,
    *,
    installer_url: str,
    sha_msi: str,
    release_date: str,
    product_code: str,
) -> str:
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
    text, product_code_count = re.subn(
        r"^(?P<indent>[ ]*)ProductCode: .*$",
        lambda match: f'{match.group("indent")}ProductCode: "{product_code}"',
        text,
        flags=re.MULTILINE,
    )
    if product_code_count != 1:
        raise ManifestFormatError(
            f"winget installer manifest needs exactly one ProductCode line, found {product_code_count}"
        )
    return text


def main() -> None:
    version = get_env("VERSION")
    sha_msi = get_env("SHA_MSI")
    release_date = get_env("RELEASE_DATE")
    product_code = read_product_code(pathlib.Path(get_env("MSI_PATH")))

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
            text = rewrite_installer(
                text,
                installer_url=installer_url,
                sha_msi=sha_msi,
                release_date=release_date,
                product_code=product_code,
            )
        if path.name.endswith("locale.en-US.yaml"):
            text = rewrite_locale(text, release_notes_url)
        path.write_text(text)
        print(f"Rewrote {path}")


if __name__ == "__main__":
    main()
