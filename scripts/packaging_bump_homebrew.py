#!/usr/bin/env python3
"""Rewrite packaging/homebrew/Casks/writ.rb with the release version and DMG SHAs."""

from __future__ import annotations

import os
import pathlib
import re
import sys

CASK_PATH = pathlib.Path("packaging/homebrew/Casks/writ.rb")


def get_env(name: str) -> str:
    value = os.environ.get(name, "")
    if not value:
        print(f"Missing environment variable: {name}", file=sys.stderr)
        sys.exit(1)
    return value


def main() -> None:
    version = get_env("VERSION")
    sha_arm64 = get_env("SHA_ARM64")
    sha_intel = get_env("SHA_INTEL")

    if not CASK_PATH.is_file():
        print(f"Cask file missing: {CASK_PATH}", file=sys.stderr)
        sys.exit(1)

    text = CASK_PATH.read_text()

    text, n_version = re.subn(
        r'version "[^"]+"',
        f'version "{version}"',
        text,
        count=1,
    )
    if n_version != 1:
        print("Failed to rewrite version field", file=sys.stderr)
        sys.exit(1)

    text, n_arm = re.subn(
        r"(on_arm do\s+sha256 )\"[^\"]+\"",
        lambda m: f'{m.group(1)}"{sha_arm64}"',
        text,
        count=1,
    )
    text, n_intel = re.subn(
        r"(on_intel do\s+sha256 )\"[^\"]+\"",
        lambda m: f'{m.group(1)}"{sha_intel}"',
        text,
        count=1,
    )
    if n_arm != 1 or n_intel != 1:
        print("Failed to rewrite SHA fields (arm/intel)", file=sys.stderr)
        sys.exit(1)

    CASK_PATH.write_text(text)
    print(f"Bumped {CASK_PATH} to {version}")


if __name__ == "__main__":
    main()
