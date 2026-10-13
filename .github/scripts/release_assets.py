#!/usr/bin/env python3
"""The files a Writ release publishes, named from its version.

The release workflow signs, checksums, attests and uploads exactly this set, so
a file no job meant to publish never reaches the release. `check` also reads
each SBOM in the set and refuses one that actions/attest would reject (no
bomFormat, specVersion or serialNumber), one that describes another project or
version, and one with no components.

Run from the repository root:
    python3 .github/scripts/release_assets.py list --version 0.6.0 --set published
    python3 .github/scripts/release_assets.py check --dir release-assets --version 0.6.0 --set published
    python3 .github/scripts/release_assets.py sbom-targets
    python3 .github/scripts/release_assets.py sbom-matrix --version 0.6.0
"""
from __future__ import annotations

import argparse
import dataclasses
import json
import pathlib
import re
import sys

SUMS = "SHA256SUMS.txt"
SUMS_SIGNATURE = "SHA256SUMS.txt.minisig"
UPDATER_MANIFEST = "latest.json"
RELEASE_META = "release-meta.json"

SBOM_CRATES = ("writ-tauri", "writ-cli")
FRONTEND_PACKAGE_NAME = "writ"
SBOM_TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
)
PLATFORMS = ("macos", "windows", "linux")
ATTEST_SBOM_MAX_BYTES = 16 * 1024 * 1024

VERSION_PATTERN = re.compile(r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?$")

SETS = ("installers", "updater-payloads", "sboms", "signing-inputs", "checksummed", "published")


class ReleaseAssetError(ValueError):
    pass


@dataclasses.dataclass(frozen=True)
class Installer:
    name: str
    platform: str
    updater_payload: bool


@dataclasses.dataclass(frozen=True)
class Sbom:
    name: str
    platforms: tuple[str, ...]
    component: str


def require_version(version: str) -> str:
    if not VERSION_PATTERN.match(version):
        raise ReleaseAssetError(f"{version!r} is not a release version such as 0.6.0 or 0.6.0-1")
    return version


def target_platform(target: str) -> str:
    if target.endswith("-apple-darwin"):
        return "macos"
    if "-windows-" in target:
        return "windows"
    if "-linux-" in target:
        return "linux"
    raise ReleaseAssetError(f"no platform for target {target}")


def installers(version: str) -> list[Installer]:
    v = require_version(version)
    return [
        Installer(f"Writ_{v}_universal.pkg", "macos", False),
        Installer(f"Writ_{v}_universal.dmg", "macos", False),
        Installer(f"Writ_{v}_universal.app.tar.gz", "macos", True),
        Installer(f"Writ_{v}_x64_en-US.msi", "windows", True),
        Installer(f"Writ_{v}_amd64.deb", "linux", True),
        Installer(f"Writ_{v}_amd64.AppImage", "linux", True),
    ]


def updater_payloads(version: str) -> list[str]:
    return [i.name for i in installers(version) if i.updater_payload]


def updater_signatures(version: str) -> list[str]:
    return [f"{name}.sig" for name in updater_payloads(version)]


def sboms(version: str) -> list[Sbom]:
    v = require_version(version)
    rust = [
        Sbom(f"{crate}_{v}_{target}.cdx.json", (target_platform(target),), crate)
        for crate in SBOM_CRATES
        for target in SBOM_TARGETS
    ]
    return rust + [Sbom(f"writ-frontend_{v}.cdx.json", PLATFORMS, FRONTEND_PACKAGE_NAME)]


def sbom_subjects(version: str, sbom: Sbom) -> list[str]:
    return [i.name for i in installers(version) if i.platform in sbom.platforms]


def asset_set(version: str, name: str) -> list[str]:
    installer_names = [i.name for i in installers(version)]
    sbom_names = [s.name for s in sboms(version)]
    if name == "installers":
        names = installer_names
    elif name == "updater-payloads":
        names = updater_payloads(version)
    elif name == "sboms":
        names = sbom_names
    elif name == "signing-inputs":
        names = installer_names + sbom_names + [RELEASE_META]
    elif name == "checksummed":
        names = installer_names + updater_signatures(version) + sbom_names
    elif name == "published":
        names = (
            installer_names
            + updater_signatures(version)
            + sbom_names
            + [SUMS, SUMS_SIGNATURE, UPDATER_MANIFEST, RELEASE_META]
        )
    else:
        raise ReleaseAssetError(f"unknown asset set {name!r}; expected one of {', '.join(SETS)}")
    return sorted(names)


@dataclasses.dataclass(frozen=True)
class SetDifference:
    missing: list[str]
    unexpected: list[str]

    def problems(self) -> list[str]:
        lines = [f"missing: {name}" for name in self.missing]
        lines += [f"not part of the release: {name}" for name in self.unexpected]
        return lines


def compare_dir(directory: pathlib.Path, expected: list[str]) -> SetDifference:
    if not directory.is_dir():
        raise ReleaseAssetError(f"{directory} is not a directory")
    present = sorted(entry.name for entry in directory.iterdir())
    non_files = sorted(entry.name for entry in directory.iterdir() if not entry.is_file() or entry.is_symlink())
    wanted = set(expected)
    return SetDifference(
        missing=sorted(wanted - set(present)),
        unexpected=sorted((set(present) - wanted) | set(non_files)),
    )


def sbom_problems(path: pathlib.Path, sbom: Sbom, version: str) -> list[str]:
    try:
        size = path.stat().st_size
        if size > ATTEST_SBOM_MAX_BYTES:
            return [f"{sbom.name} is {size} bytes; actions/attest takes at most {ATTEST_SBOM_MAX_BYTES}"]
        document = json.loads(path.read_text(encoding="utf-8"))
    except OSError as error:
        return [f"{sbom.name} cannot be read: {error}"]
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        return [f"{sbom.name} is not UTF-8 JSON: {error}"]
    if not isinstance(document, dict):
        return [f"{sbom.name} is not a JSON object"]
    found: list[str] = []
    if document.get("bomFormat") != "CycloneDX":
        found.append(f"{sbom.name} has bomFormat {document.get('bomFormat')!r}, expected 'CycloneDX'")
    for field in ("specVersion", "serialNumber"):
        value = document.get(field)
        if not isinstance(value, str) or not value:
            found.append(f"{sbom.name} has no {field}, which actions/attest requires of a CycloneDX SBOM")
    metadata = document.get("metadata")
    component = metadata.get("component") if isinstance(metadata, dict) else None
    if not isinstance(component, dict):
        found.append(f"{sbom.name} names no metadata.component")
    else:
        if component.get("name") != sbom.component:
            found.append(f"{sbom.name} describes {component.get('name')!r}, expected {sbom.component!r}")
        if component.get("version") != version:
            found.append(f"{sbom.name} describes version {component.get('version')!r}, expected {version!r}")
    components = document.get("components")
    if not isinstance(components, list) or not components:
        found.append(f"{sbom.name} lists no components")
    return found


def content_problems(directory: pathlib.Path, version: str, set_name: str) -> list[str]:
    wanted = set(asset_set(version, set_name))
    found: list[str] = []
    for sbom in sboms(version):
        path = directory / sbom.name
        if sbom.name in wanted and path.is_file() and not path.is_symlink():
            found += sbom_problems(path, sbom, version)
    return found


def sbom_matrix(version: str) -> list[dict[str, str]]:
    return [{"sbom": s.name, "subjects": " ".join(sbom_subjects(version, s))} for s in sboms(version)]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)

    list_cmd = sub.add_parser("list", help="print one asset set, one name per line")
    list_cmd.add_argument("--version", required=True)
    list_cmd.add_argument("--set", required=True, choices=SETS)

    check_cmd = sub.add_parser("check", help="require a directory to hold exactly one asset set, its SBOMs well formed")
    check_cmd.add_argument("--dir", required=True, type=pathlib.Path)
    check_cmd.add_argument("--version", required=True)
    check_cmd.add_argument("--set", required=True, choices=SETS)

    sub.add_parser("sbom-targets", help="print the Rust targets that get an SBOM")

    matrix_cmd = sub.add_parser("sbom-matrix", help="print the SBOM attestation matrix as JSON")
    matrix_cmd.add_argument("--version", required=True)

    args = parser.parse_args(argv)
    try:
        if args.command == "list":
            print("\n".join(asset_set(args.version, args.set)))
        elif args.command == "check":
            difference = compare_dir(args.dir, asset_set(args.version, args.set))
            found = difference.problems() + content_problems(args.dir, args.version, args.set)
            if found:
                print(f"{args.dir} does not hold the {args.set} set of release {args.version}:", file=sys.stderr)
                for line in found:
                    print(f"  {line}", file=sys.stderr)
                return 1
            print(f"{args.dir} holds the {args.set} set of release {args.version}")
        elif args.command == "sbom-targets":
            print("\n".join(SBOM_TARGETS))
        elif args.command == "sbom-matrix":
            print(json.dumps(sbom_matrix(args.version), separators=(",", ":")))
    except ReleaseAssetError as error:
        print(f"release_assets: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
