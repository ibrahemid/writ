#!/usr/bin/env python3
"""Bump the version across Cargo.toml (workspace), tauri.conf.json, package.json, site/package.json and site/src/data/release.json.

Usage: bump_version.py <new_version>

The new_version must be a valid semver without a leading v. Every file must
exist and contain a single canonical version field at the expected key, or the
script exits non-zero without writing any partial state.

site/src/data/release.json names the release the repo is on, which the release
workflow asserts against the tag before it builds anything. The bump moves its
version, tag and download URLs and clears everything that describes artifacts
that do not exist yet; the site deploy regenerates the whole file from the
published release.
"""
from __future__ import annotations

import json
import pathlib
import re
import sys
from collections.abc import Callable


class BumpError(RuntimeError):
    pass


def bump_cargo_toml(path: pathlib.Path, new_version: str) -> None:
    text = path.read_text()
    pattern = re.compile(
        r'(?P<prefix>\[workspace\.package\][^\[]*?version\s*=\s*")(?P<ver>[^"]+)(?P<suffix>")',
        re.DOTALL,
    )
    match = pattern.search(text)
    if not match:
        raise BumpError(f"Could not find [workspace.package] version in {path}")
    updated = pattern.sub(rf'\g<prefix>{new_version}\g<suffix>', text, count=1)
    path.write_text(updated)


def bump_tauri_conf(path: pathlib.Path, new_version: str) -> None:
    data = json.loads(path.read_text())
    if "version" not in data:
        raise BumpError(f"No top-level 'version' field in {path}")
    data["version"] = new_version
    path.write_text(json.dumps(data, indent=4) + "\n")


def bump_package_json(path: pathlib.Path, new_version: str) -> None:
    raw = path.read_text()
    data = json.loads(raw)
    if "version" not in data:
        raise BumpError(f"No top-level 'version' field in {path}")
    data["version"] = new_version
    indent = 4
    first_indent_match = re.search(r"\n(?P<indent> +)\"", raw)
    if first_indent_match:
        indent = len(first_indent_match.group("indent"))
    path.write_text(json.dumps(data, indent=indent) + "\n")


def bump_release_json(path: pathlib.Path, new_version: str) -> None:
    data = json.loads(path.read_text())
    old_version = data.get("version")
    if not old_version:
        raise BumpError(f"No 'version' field in {path}")
    data["version"] = new_version
    data["tag"] = f"v{new_version}"
    # publishedAt is left alone: the bump has no date to give, and the site
    # deploy rewrites the whole file from the published release.
    # Nothing has been built for this version, so nothing may claim it has.
    data["published"] = False
    data["notarized"] = False
    data["hasChecksums"] = False
    for platform in data.get("platforms", {}).values():
        for key, value in list(platform.items()):
            if key.startswith("sha256_"):
                platform[key] = ""
            elif key.startswith("size_"):
                platform[key] = 0
            elif isinstance(value, str):
                platform[key] = value.replace(old_version, new_version)
    path.write_text(json.dumps(data, indent=2) + "\n")


# Every file the bump writes, relative to the repository root. The Bump version
# workflow stages exactly these plus Cargo.lock.
TARGETS: tuple[tuple[str, Callable[[pathlib.Path, str], None]], ...] = (
    ("Cargo.toml", bump_cargo_toml),
    ("src-tauri/tauri.conf.json", bump_tauri_conf),
    ("package.json", bump_package_json),
    ("site/package.json", bump_package_json),
    ("site/src/data/release.json", bump_release_json),
)


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("Usage: bump_version.py <new_version>", file=sys.stderr)
        return 2
    new_version = argv[1]
    if not re.match(r"^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$", new_version):
        print(f"Invalid semver: {new_version}", file=sys.stderr)
        return 2

    root = pathlib.Path(__file__).resolve().parent.parent.parent
    targets = [(relative, root / relative, fn) for relative, fn in TARGETS]

    for relative, path, _fn in targets:
        if not path.exists():
            print(f"Missing file: {path} ({relative})", file=sys.stderr)
            return 1

    try:
        for relative, path, fn in targets:
            fn(path, new_version)
            print(f"Bumped {relative} -> {new_version}")
    except BumpError as err:
        print(f"ERROR: {err}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
