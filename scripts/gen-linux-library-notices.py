#!/usr/bin/env python3
"""Credit the shared libraries the Linux AppImage carries.

The AppImage bundles libraries from the build machine (Ubuntu 22.04): the GTK
and WebKitGTK stack, LGPL among them. `append` lists the AppImage's `usr/lib`,
asks dpkg which Ubuntu package owns each library, and appends a section to the
notices file: one row per library with its package, source package, version
and licence, the offer of the corresponding source, each package's Debian
copyright file, and every licence text those files point to under
/usr/share/common-licenses. The release then bundles the AppImage again so it
ships the section. `check` reads the notices file inside an AppImage and fails
unless its table lists exactly the libraries that AppImage carries.

Usage (on the Ubuntu runner that built the AppImage):
    scripts/gen-linux-library-notices.py append --appimage A.AppImage --notices THIRD-PARTY-NOTICES.md
    scripts/gen-linux-library-notices.py check --appimage A.AppImage
"""

from __future__ import annotations

import argparse
import dataclasses
import os
import pathlib
import re
import shutil
import struct
import subprocess
import sys
import tempfile
from typing import Protocol

SECTION_HEADING = "## Linux system libraries in the AppImage"
NOTICES_NAME = "THIRD-PARTY-NOTICES.md"
DOC_ROOT = pathlib.Path("/usr/share/doc")
COMMON_LICENCES = pathlib.Path("/usr/share/common-licenses")
SOURCE_REQUESTS = "https://github.com/ibrahemid/writ/issues"
LIBRARY_NAME = re.compile(r"\.so(\.\d+)*$")
LIBRARY_DIRS = ("/usr/lib/x86_64-linux-gnu", "/lib/x86_64-linux-gnu", "/usr/lib", "/lib")
COMMON_LICENCE_REFERENCE = re.compile(r"/usr/share/common-licenses/([A-Za-z0-9.+_-]+)")
TABLE_ROW = re.compile(r"^\| `(usr/lib/[^`]+)` \|", re.MULTILINE)


class NoticeError(RuntimeError):
    """A library cannot be credited, or an AppImage disagrees with its notices."""


class PackageDatabase(Protocol):
    def owner(self, path: str) -> str | None: ...

    def source(self, package: str) -> tuple[str, str]: ...


class Dpkg:
    """dpkg's database on the machine that built the AppImage."""

    def owner(self, path: str) -> str | None:
        result = subprocess.run(["dpkg-query", "-S", path], capture_output=True, text=True)
        if result.returncode != 0:
            return None
        for line in result.stdout.splitlines():
            packages, _, owned = line.partition(": ")
            if owned.strip() == path and "," not in packages:
                return packages.split(":")[0].strip()
        return None

    def source(self, package: str) -> tuple[str, str]:
        result = subprocess.run(
            ["dpkg-query", "-W", "-f=${source:Package}\t${source:Version}", package],
            capture_output=True,
            text=True,
        )
        if result.returncode != 0 or "\t" not in result.stdout:
            raise NoticeError(f"dpkg-query has no source package for {package}: {result.stderr.strip()}")
        name, version = result.stdout.strip().split("\t", 1)
        return name, version


@dataclasses.dataclass(frozen=True)
class Library:
    path: str
    package: str
    source: str
    version: str


def squashfs_offset(appimage: pathlib.Path) -> int:
    """Where the AppImage's squashfs starts: after the runtime's ELF section headers."""
    with appimage.open("rb") as handle:
        header = handle.read(64)
    if len(header) < 64 or header[:4] != b"\x7fELF" or header[4] != 2:
        raise NoticeError(f"{appimage} is not a 64-bit ELF AppImage")
    section_offset = struct.unpack_from("<Q", header, 40)[0]
    entry_size, entry_count = struct.unpack_from("<HH", header, 58)
    return section_offset + entry_size * entry_count


def appimage_listing(appimage: pathlib.Path) -> str:
    offset = squashfs_offset(appimage)
    result = subprocess.run(
        ["unsquashfs", "-lln", "-o", str(offset), str(appimage)], capture_output=True, text=True
    )
    if result.returncode != 0:
        raise NoticeError(f"unsquashfs could not list {appimage}: {result.stderr.strip()}")
    return result.stdout


def shared_libraries(listing: str) -> list[str]:
    """Regular files under usr/lib named like a shared library, from `unsquashfs -lln`.

    Links are left out: each points at a file listed on its own.
    """
    found = set()
    for line in listing.splitlines():
        fields = line.split(maxsplit=5)
        if len(fields) < 6 or not fields[0].startswith("-"):
            continue
        path = fields[5].removeprefix("squashfs-root/")
        if path.startswith("usr/lib/") and LIBRARY_NAME.search(path):
            found.add(path)
    return sorted(found)


def host_candidates(path: str) -> list[str]:
    """Where the build machine keeps the file the AppImage copied to `path`."""
    name = path.rsplit("/", 1)[-1]
    candidates = ["/" + path] + [f"{directory}/{name}" for directory in LIBRARY_DIRS]
    expanded: list[str] = []
    for candidate in candidates:
        resolved = os.path.realpath(candidate) if os.path.exists(candidate) else None
        for item in (candidate, resolved):
            if not item:
                continue
            for variant in (item, usr_twin(item)):
                if variant and variant not in expanded:
                    expanded.append(variant)
    return expanded


def usr_twin(path: str) -> str | None:
    """The same path across the /lib and /usr/lib merge, which dpkg records either way."""
    if path.startswith("/usr/lib/"):
        return path[len("/usr"):]
    if path.startswith("/lib/"):
        return "/usr" + path
    return None


def map_libraries(paths: list[str], database: PackageDatabase) -> list[Library]:
    libraries = []
    unowned = []
    for path in paths:
        package = next(
            (owner for owner in map(database.owner, host_candidates(path)) if owner), None
        )
        if package is None:
            unowned.append(path)
            continue
        source, version = database.source(package)
        libraries.append(Library(path, package, source, version))
    if unowned:
        raise NoticeError(
            "no Ubuntu package owns these AppImage libraries, so they cannot be credited: "
            + ", ".join(unowned)
        )
    return libraries


def copyright_text(package: str, doc_root: pathlib.Path) -> str:
    path = doc_root / package / "copyright"
    if not path.is_file():
        raise NoticeError(f"{package} has no copyright file at {path}")
    return path.read_text(errors="replace").replace("\r\n", "\n").strip("\n")


def package_licence(text: str) -> str:
    """The licence a machine-readable Debian copyright file gives `Files: *`."""
    if not text.startswith("Format:"):
        return "see the copyright file"
    for paragraph in re.split(r"\n\s*\n", text):
        fields = dict(
            (match.group(1).lower(), match.group(2).strip())
            for match in re.finditer(r"^([A-Za-z-]+):[ \t]*(.*)$", paragraph, re.MULTILINE)
        )
        if fields.get("files", "").split() == ["*"] and fields.get("license"):
            return fields["license"]
    return "see the copyright file"


def fence_for(text: str) -> str:
    longest = max((len(line) - len(line.lstrip("`")) for line in text.split("\n")), default=0)
    return "`" * max(3, longest + 1)


def fenced(text: str) -> list[str]:
    fence = fence_for(text)
    return [fence, text, fence, ""]


def render_section(
    libraries: list[Library], doc_root: pathlib.Path, common_licences: pathlib.Path
) -> str:
    copyrights = {lib.package: copyright_text(lib.package, doc_root) for lib in libraries}
    lines = [
        SECTION_HEADING,
        "",
        "The Linux AppImage carries these shared libraries from Ubuntu 22.04, the system it "
        "is built on. Each row names the Ubuntu package the file comes from, its source "
        "package and version, and the licence the package's Debian copyright file gives its "
        "files as a whole. The copyright files below give the licence of every part.",
        "",
        "| Library | Package | Source package | Licence |",
        "| --- | --- | --- | --- |",
    ]
    for lib in libraries:
        licence = package_licence(copyrights[lib.package]).replace("|", "\\|")
        lines.append(f"| `{lib.path}` | {lib.package} | {lib.source} {lib.version} | {licence} |")
    lines.append("")

    sources = sorted({(lib.source, lib.version) for lib in libraries})
    lines += [
        "### Source code",
        "",
        "For three years from the date this AppImage was published, the complete "
        "corresponding source code of every library above, at the version listed, is "
        "available from the Writ maintainer for no more than the cost of providing it. "
        f"Ask at {SOURCE_REQUESTS}. Ubuntu publishes the same sources: on Ubuntu 22.04, "
        "`apt-get source <package>=<version>` fetches them.",
        "",
    ]
    lines += [f"- {name} {version}: https://launchpad.net/ubuntu/+source/{name}/{version}" for name, version in sources]
    lines.append("")

    lines += ["### Debian copyright files", ""]
    for package in sorted(copyrights):
        lines += [f"#### {package}", ""] + fenced(copyrights[package])

    referenced = sorted(
        {name for text in copyrights.values() for name in COMMON_LICENCE_REFERENCE.findall(text)}
    )
    texts = []
    for name in referenced:
        path = common_licences / name.rstrip(".")
        if path.is_file():
            texts.append((name.rstrip("."), path.read_text(errors="replace").strip("\n")))
    if texts:
        lines += ["### Licence texts", "", "The copyright files above refer to these texts.", ""]
        for name, text in texts:
            lines += [f"#### {name}", ""] + fenced(text)
    return "\n".join(lines).rstrip("\n") + "\n"


def append_section(notices: pathlib.Path, section: str) -> None:
    current = notices.read_text()
    if SECTION_HEADING in current:
        raise NoticeError(f"{notices} already has the Linux system library section")
    notices.write_text(current.rstrip("\n") + "\n\n" + section)


def credited_libraries(notices_text: str) -> list[str]:
    if SECTION_HEADING not in notices_text:
        return []
    section = notices_text.split(SECTION_HEADING, 1)[1]
    return sorted(set(TABLE_ROW.findall(section)))


def notices_in_appimage(appimage: pathlib.Path, listing: str) -> str:
    paths = [
        fields[5].removeprefix("squashfs-root/")
        for fields in (line.split(maxsplit=5) for line in listing.splitlines())
        if len(fields) == 6 and fields[0].startswith("-") and fields[5].endswith("/" + NOTICES_NAME)
    ]
    if len(paths) != 1:
        raise NoticeError(f"{appimage} holds {len(paths)} copies of {NOTICES_NAME}, expected one")
    with tempfile.TemporaryDirectory() as tmp:
        result = subprocess.run(
            ["unsquashfs", "-o", str(squashfs_offset(appimage)), "-d", f"{tmp}/root", str(appimage), paths[0]],
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            raise NoticeError(f"unsquashfs could not extract {paths[0]}: {result.stderr.strip()}")
        return (pathlib.Path(tmp) / "root" / paths[0]).read_text()


def mismatch(shipped: list[str], credited: list[str]) -> list[str]:
    found = [f"not credited: {path}" for path in sorted(set(shipped) - set(credited))]
    found += [f"credited but not shipped: {path}" for path in sorted(set(credited) - set(shipped))]
    return found


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    append = commands.add_parser("append", help="append the section to a notices file")
    append.add_argument("--appimage", type=pathlib.Path, required=True)
    append.add_argument("--notices", type=pathlib.Path, required=True)
    check = commands.add_parser("check", help="check the notices inside an AppImage")
    check.add_argument("--appimage", type=pathlib.Path, required=True)
    args = parser.parse_args(argv)

    try:
        listing = appimage_listing(args.appimage)
        shipped = shared_libraries(listing)
        if not shipped:
            raise NoticeError(f"{args.appimage} lists no shared libraries under usr/lib")
        if args.command == "append":
            if shutil.which("dpkg-query") is None:
                raise NoticeError("dpkg-query is not on PATH; run this on the Ubuntu machine that built the AppImage")
            section = render_section(map_libraries(shipped, Dpkg()), DOC_ROOT, COMMON_LICENCES)
            append_section(args.notices, section)
            print(f"linux library notices: credited {len(shipped)} libraries in {args.notices}")
            return 0
        problems = mismatch(shipped, credited_libraries(notices_in_appimage(args.appimage, listing)))
    except NoticeError as error:
        print(f"linux library notices: {error}", file=sys.stderr)
        return 1
    if problems:
        for line in problems:
            print(f"linux library notices: {line}", file=sys.stderr)
        return 1
    print(f"linux library notices: the AppImage credits all {len(shipped)} libraries it ships")
    return 0


if __name__ == "__main__":
    sys.exit(main())
