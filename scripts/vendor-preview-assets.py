#!/usr/bin/env python3
"""Copy the preview's Mermaid and KaTeX builds from node_modules into src-tauri/assets.

package.json pins both as devDependencies, so Dependabot and `pnpm audit` see
the versions the preview ships. The renderers embed the copies with
`include_bytes!`, so the copies are committed. `--check` fails when a copy
differs from the installed package, when the assets hold a font the package no
longer ships, or when licences/notices.toml names another version or leaves a
copied file out.

Usage:
    scripts/vendor-preview-assets.py          copy, then check
    scripts/vendor-preview-assets.py --check  exit 1 if anything is stale
"""

from __future__ import annotations

import argparse
import dataclasses
import json
import pathlib
import shutil
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent


@dataclasses.dataclass(frozen=True)
class Runtime:
    package: str
    display: str
    files: tuple[tuple[str, str], ...]
    font_dir: tuple[str, str] | None = None


RUNTIMES = (
    Runtime(
        package="katex",
        display="KaTeX",
        files=(
            ("dist/katex.min.js", "katex/katex.min.js"),
            ("dist/katex.min.css", "katex/katex.min.css"),
        ),
        font_dir=("dist/fonts", "katex/fonts"),
    ),
    Runtime(
        package="mermaid",
        display="Mermaid",
        files=(("dist/mermaid.min.js", "mermaid/mermaid.min.js"),),
    ),
)

FONT_SUFFIX = ".woff2"


class VendorError(RuntimeError):
    """A package is missing or the repository disagrees with it."""


def assets_dir(root: pathlib.Path) -> pathlib.Path:
    return root / "src-tauri" / "assets"


def package_dir(root: pathlib.Path, runtime: Runtime) -> pathlib.Path:
    path = root / "node_modules" / runtime.package
    if not (path / "package.json").is_file():
        raise VendorError(f"node_modules/{runtime.package} is missing; run pnpm install")
    return path


def installed_version(root: pathlib.Path, runtime: Runtime) -> str:
    manifest = json.loads((package_dir(root, runtime) / "package.json").read_text())
    return str(manifest["version"])


def pinned_version(root: pathlib.Path, runtime: Runtime) -> str:
    manifest = json.loads((root / "package.json").read_text())
    pinned = manifest.get("devDependencies", {}).get(runtime.package)
    if not isinstance(pinned, str):
        raise VendorError(f"package.json does not pin {runtime.package} as a devDependency")
    return pinned


def planned_copies(root: pathlib.Path, runtime: Runtime) -> list[tuple[pathlib.Path, pathlib.Path]]:
    """Source and destination of every file the runtime ships."""
    source_root = package_dir(root, runtime)
    copies = []
    for source, destination in runtime.files:
        path = source_root / source
        if not path.is_file():
            raise VendorError(f"{runtime.package} {installed_version(root, runtime)} has no {source}")
        copies.append((path, assets_dir(root) / destination))
    if runtime.font_dir is not None:
        source, destination = runtime.font_dir
        fonts = sorted((source_root / source).glob(f"*{FONT_SUFFIX}"))
        if not fonts:
            raise VendorError(f"{runtime.package} has no {FONT_SUFFIX} fonts under {source}")
        copies.extend((font, assets_dir(root) / destination / font.name) for font in fonts)
    return copies


def stale_fonts(root: pathlib.Path, runtime: Runtime) -> list[pathlib.Path]:
    """Fonts in the assets that the package no longer ships."""
    if runtime.font_dir is None:
        return []
    shipped = {destination for _, destination in planned_copies(root, runtime)}
    font_dir = assets_dir(root) / runtime.font_dir[1]
    return sorted(path for path in font_dir.glob(f"*{FONT_SUFFIX}") if path not in shipped)


def bundled_entry(root: pathlib.Path, runtime: Runtime) -> dict[str, object] | None:
    with (root / "licences" / "notices.toml").open("rb") as handle:
        config = tomllib.load(handle)
    for entry in config.get("bundled", []):
        if str(entry.get("name", "")).split(" ")[0] == runtime.display:
            return entry
    return None


def problems(root: pathlib.Path) -> list[str]:
    found: list[str] = []
    for runtime in RUNTIMES:
        version = installed_version(root, runtime)
        pinned = pinned_version(root, runtime)
        if pinned != version:
            found.append(
                f"package.json pins {runtime.package} {pinned} but node_modules holds {version}; "
                "pin an exact version and run pnpm install"
            )
        for source, destination in planned_copies(root, runtime):
            relative = destination.relative_to(root)
            if not destination.is_file():
                found.append(f"{relative} is missing")
            elif destination.read_bytes() != source.read_bytes():
                found.append(f"{relative} differs from {runtime.package} {version}")
        for path in stale_fonts(root, runtime):
            found.append(f"{path.relative_to(root)} is not shipped by {runtime.package} {version}")
        entry = bundled_entry(root, runtime)
        if entry is None:
            found.append(f"licences/notices.toml has no [[bundled]] entry for {runtime.display}")
            continue
        expected_name = f"{runtime.display} {version}"
        if entry.get("name") != expected_name:
            found.append(f"licences/notices.toml names {entry.get('name')!r}, expected {expected_name!r}")
        listed = set(entry.get("files", []))  # type: ignore[arg-type]
        for _, destination in planned_copies(root, runtime):
            relative = destination.relative_to(root).as_posix()
            if relative not in listed:
                found.append(f"licences/notices.toml leaves {relative} out of {expected_name}")
    return found


def copy(root: pathlib.Path) -> None:
    for runtime in RUNTIMES:
        for source, destination in planned_copies(root, runtime):
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, destination)
        for path in stale_fonts(root, runtime):
            path.unlink()


def main(argv: list[str] | None = None, root: pathlib.Path = ROOT) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail when a copy or the notices are stale")
    args = parser.parse_args(argv)
    try:
        if not args.check:
            copy(root)
        found = problems(root)
    except VendorError as error:
        print(f"preview assets: {error}", file=sys.stderr)
        return 1
    if found:
        for line in found:
            print(f"preview assets: {line}", file=sys.stderr)
        if args.check:
            print("preview assets: run scripts/vendor-preview-assets.py and commit the result", file=sys.stderr)
        return 1
    print("preview assets: the copies match their packages")
    return 0


if __name__ == "__main__":
    sys.exit(main())
