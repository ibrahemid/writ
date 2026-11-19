#!/usr/bin/env python3
"""Generate src-tauri/wix/legacy-progids.wxs from the file associations in src-tauri/tauri.conf.json
and the main binary name in src-tauri/Cargo.toml.

Usage:
    scripts/gen-legacy-progids.py            write the fragment
    scripts/gen-legacy-progids.py --check    exit 1 if the fragment is stale
"""

from __future__ import annotations

import argparse
import difflib
import json
import pathlib
import re
import sys
import tomllib
from collections.abc import Sequence
from dataclasses import dataclass

ROOT = pathlib.Path(__file__).resolve().parent.parent
TAURI_CONF = ROOT / "src-tauri" / "tauri.conf.json"
CARGO_MANIFEST = ROOT / "src-tauri" / "Cargo.toml"
OUTPUT_PATH = ROOT / "src-tauri" / "wix" / "legacy-progids.wxs"

LEGACY_PRODUCT_NAME = "Writ"
COMPONENT_ID = "LegacyProgIds"
COMPONENT_GUID = "E808E13F-A233-4FE4-929B-FB5AA08886C0"
# ICE69 rejects a [#Path] reference here: the template keeps that file in features this component is not in.
INSTALL_DIRECTORY_ID = "INSTALLDIR"

EXTENSION_PATTERN = re.compile(r"[A-Za-z0-9][A-Za-z0-9._+-]*")
BINARY_NAME_PATTERN = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*")
FORMATTED_SPECIAL = re.compile(r"[\[\]{}]")
INDENT = "    "


class GenerationError(RuntimeError):
    """The configuration cannot be turned into a fragment."""


@dataclass(frozen=True)
class Association:
    ext: str
    description: str | None


def escape_attribute(value: str) -> str:
    return (
        value.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


def escape_formatted_literal(value: str) -> str:
    return FORMATTED_SPECIAL.sub(lambda match: f"[\\{match.group(0)}]", value)


def get_product_name(conf: dict[str, object]) -> str:
    name = conf.get("productName")
    if not isinstance(name, str) or not name.strip():
        raise GenerationError("tauri.conf.json has no productName")
    return name


def get_main_binary_name(conf: dict[str, object], manifest: dict[str, object]) -> str:
    name = conf.get("mainBinaryName")
    source = "tauri.conf.json mainBinaryName"
    if name is None:
        package = manifest.get("package")
        name = package.get("name") if isinstance(package, dict) else None
        source = "Cargo.toml package.name"
    if not isinstance(name, str) or not BINARY_NAME_PATTERN.fullmatch(name):
        raise GenerationError(f"{source} is {name!r}, which cannot name the main binary in an open command")
    return name


def get_open_command(main_binary_name: str) -> str:
    return f'"[{INSTALL_DIRECTORY_ID}]{main_binary_name}.exe" "%1"'


def get_associations(conf: dict[str, object]) -> list[Association]:
    bundle = conf.get("bundle")
    if not isinstance(bundle, dict):
        raise GenerationError("tauri.conf.json has no bundle section")
    entries = bundle.get("fileAssociations")
    if not isinstance(entries, list) or not entries:
        raise GenerationError("tauri.conf.json has no bundle.fileAssociations")

    associations: list[Association] = []
    seen: dict[str, int] = {}
    for index, entry in enumerate(entries):
        if not isinstance(entry, dict):
            raise GenerationError(f"bundle.fileAssociations[{index}] is not an object")
        exts = entry.get("ext")
        if not isinstance(exts, list):
            raise GenerationError(f"bundle.fileAssociations[{index}].ext is not a list")
        description = entry.get("description")
        if description is not None and not isinstance(description, str):
            raise GenerationError(f"bundle.fileAssociations[{index}].description is not a string")
        for raw in exts:
            if not isinstance(raw, str):
                raise GenerationError(f"bundle.fileAssociations[{index}].ext holds a non-string value")
            ext = raw[1:] if raw.startswith(".") else raw
            if not EXTENSION_PATTERN.fullmatch(ext):
                raise GenerationError(
                    f"bundle.fileAssociations[{index}].ext has {raw!r}, which cannot name a registry key"
                )
            key = ext.lower()
            if key in seen:
                raise GenerationError(
                    f"extension {ext!r} appears in bundle.fileAssociations[{seen[key]}] and [{index}]"
                )
            seen[key] = index
            associations.append(Association(ext=ext, description=description or None))
    return associations


def render_fragment(product_name: str, main_binary_name: str, associations: Sequence[Association]) -> str:
    if not associations:
        raise GenerationError("no file associations to register")

    verb_text = escape_attribute(escape_formatted_literal(f"Open with {product_name}"))
    command = escape_attribute(get_open_command(main_binary_name))
    lines = [
        '<?xml version="1.0" encoding="utf-8"?>',
        '<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">',
        f"{INDENT}<Fragment>",
        f'{INDENT * 2}<DirectoryRef Id="INSTALLDIR">',
        f'{INDENT * 3}<Component Id="{COMPONENT_ID}" Guid="{COMPONENT_GUID}">',
    ]
    for position, association in enumerate(associations):
        key_path = ' KeyPath="yes"' if position == 0 else ""
        prog_id = f"{LEGACY_PRODUCT_NAME}.{association.ext}"
        lines.append(f'{INDENT * 4}<RegistryKey Root="HKLM" Key="Software\\Classes\\{prog_id}">')
        if association.description:
            description = escape_attribute(escape_formatted_literal(association.description))
            lines.append(f'{INDENT * 5}<RegistryValue Type="string" Value="{description}" />')
        lines.extend(
            [
                f'{INDENT * 5}<RegistryKey Key="shell\\open">',
                f'{INDENT * 6}<RegistryValue Type="string" Value="{verb_text}" />',
                f'{INDENT * 6}<RegistryKey Key="command">',
                f'{INDENT * 7}<RegistryValue Type="string" Value="{command}"{key_path} />',
                f"{INDENT * 6}</RegistryKey>",
                f"{INDENT * 5}</RegistryKey>",
                f"{INDENT * 4}</RegistryKey>",
            ]
        )
    lines.extend(
        [
            f"{INDENT * 3}</Component>",
            f"{INDENT * 2}</DirectoryRef>",
            f"{INDENT}</Fragment>",
            "</Wix>",
        ]
    )
    document = "\n".join(lines) + "\n"
    if "{{" in document:
        raise GenerationError("the fragment holds '{{', which the bundler's template pass would consume")
    return document


def generate(conf_path: pathlib.Path, manifest_path: pathlib.Path = CARGO_MANIFEST) -> str:
    try:
        conf = json.loads(conf_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise GenerationError(f"cannot read {conf_path}: {exc}") from exc
    if not isinstance(conf, dict):
        raise GenerationError(f"{conf_path} is not a JSON object")
    try:
        manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise GenerationError(f"cannot read {manifest_path}: {exc}") from exc
    return render_fragment(get_product_name(conf), get_main_binary_name(conf, manifest), get_associations(conf))


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help=f"fail when {OUTPUT_PATH.name} differs from a fresh generation",
    )
    args = parser.parse_args(argv)

    try:
        content = generate(TAURI_CONF)
    except GenerationError as exc:
        print(f"legacy progids: {exc}", file=sys.stderr)
        return 1

    if args.check:
        if not OUTPUT_PATH.is_file():
            print(
                f"legacy progids: {OUTPUT_PATH.name} is missing; run scripts/gen-legacy-progids.py",
                file=sys.stderr,
            )
            return 1
        current = OUTPUT_PATH.read_text(encoding="utf-8")
        if current != content:
            diff = difflib.unified_diff(
                current.splitlines(keepends=True),
                content.splitlines(keepends=True),
                fromfile=f"{OUTPUT_PATH.name} (committed)",
                tofile=f"{OUTPUT_PATH.name} (regenerated)",
            )
            sys.stderr.writelines(diff)
            print(
                f"\nlegacy progids: {OUTPUT_PATH.name} is out of date; run "
                "scripts/gen-legacy-progids.py and commit the result",
                file=sys.stderr,
            )
            return 1
        print(f"legacy progids: {OUTPUT_PATH.name} is up to date")
        return 0

    OUTPUT_PATH.write_text(content, encoding="utf-8")
    print(f"legacy progids: wrote {OUTPUT_PATH.name} ({len(content)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
