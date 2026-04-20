#!/usr/bin/env python3
"""Generate latest.json for the Tauri updater from a directory of release assets.

Expected environment variables:
  VERSION  semver version without leading v (e.g. 0.1.0)
  TAG      git tag (e.g. v0.1.0)
  REPO     "<owner>/<repo>" GitHub repository identifier

The script is run from the directory containing downloaded release assets.
Tauri's updater expects the following platform keys:
  darwin-aarch64, darwin-x86_64, windows-x86_64, linux-x86_64
Each platform entry must contain a signed URL and the .sig content.
"""
from __future__ import annotations

import datetime as _dt
import json
import os
import pathlib
import re
import sys


def match_one(patterns: list[str], root: pathlib.Path) -> pathlib.Path | None:
    for pattern in patterns:
        rx = re.compile(pattern)
        for entry in sorted(root.iterdir()):
            if entry.is_file() and rx.search(entry.name):
                return entry
    return None


def read_sig(asset: pathlib.Path) -> str:
    sig = asset.with_suffix(asset.suffix + ".sig")
    if sig.exists():
        return sig.read_text().strip()
    return ""


def main() -> int:
    try:
        version = os.environ["VERSION"]
        tag = os.environ["TAG"]
        repo = os.environ["REPO"]
    except KeyError as missing:
        print(f"Missing required env var: {missing}", file=sys.stderr)
        return 1

    root = pathlib.Path(".")
    platforms: dict[str, dict[str, str]] = {}

    mac_asset = match_one([r"\.app\.tar\.gz$"], root)
    if mac_asset:
        url = f"https://github.com/{repo}/releases/download/{tag}/{mac_asset.name}"
        signature = read_sig(mac_asset)
        platforms["darwin-aarch64"] = {"signature": signature, "url": url}
        platforms["darwin-x86_64"] = {"signature": signature, "url": url}

    windows_asset = match_one(
        [r"_x64-setup\.nsis\.zip$", r"_x64-setup\.exe\.zip$", r"_en-US\.msi\.zip$", r"\.msi\.zip$"],
        root,
    )
    if windows_asset:
        platforms["windows-x86_64"] = {
            "signature": read_sig(windows_asset),
            "url": f"https://github.com/{repo}/releases/download/{tag}/{windows_asset.name}",
        }

    linux_asset = match_one([r"\.AppImage\.tar\.gz$"], root)
    if linux_asset:
        platforms["linux-x86_64"] = {
            "signature": read_sig(linux_asset),
            "url": f"https://github.com/{repo}/releases/download/{tag}/{linux_asset.name}",
        }

    manifest = {
        "version": version,
        "notes": f"See https://github.com/{repo}/releases/tag/{tag}",
        "pub_date": _dt.datetime.now(_dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": platforms,
    }

    output = pathlib.Path("latest.json")
    output.write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))

    if not platforms:
        print(
            "WARNING: no updater platforms matched any asset. "
            "latest.json was written with an empty platforms object.",
            file=sys.stderr,
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
