#!/usr/bin/env python3
"""Bump the version across Cargo.toml (workspace), tauri.conf.json, and package.json.

Usage: bump_version.py <new_version>

The new_version must be a valid semver without a leading v. All three files must
exist and contain a single canonical version field at the expected key, or the
script exits non-zero without writing any partial state.
"""
from __future__ import annotations

import json
import pathlib
import re
import sys


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


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("Usage: bump_version.py <new_version>", file=sys.stderr)
        return 2
    new_version = argv[1]
    if not re.match(r"^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$", new_version):
        print(f"Invalid semver: {new_version}", file=sys.stderr)
        return 2

    root = pathlib.Path(__file__).resolve().parent.parent.parent
    targets = {
        "Cargo.toml": (root / "Cargo.toml", bump_cargo_toml),
        "tauri.conf.json": (root / "src-tauri" / "tauri.conf.json", bump_tauri_conf),
        "package.json": (root / "package.json", bump_package_json),
    }

    for label, (path, _fn) in targets.items():
        if not path.exists():
            print(f"Missing file: {path} ({label})", file=sys.stderr)
            return 1

    try:
        for label, (path, fn) in targets.items():
            fn(path, new_version)
            print(f"Bumped {label} -> {new_version}")
    except BumpError as err:
        print(f"ERROR: {err}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
