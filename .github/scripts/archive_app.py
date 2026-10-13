#!/usr/bin/env python3
"""Packs a signed macOS .app into the archive tauri-plugin-updater installs.

The updater drops the first path component of every entry and installs the rest
as the new bundle, so the archive holds exactly one top-level directory, the
.app, with no AppleDouble or extended-attribute entries beside it.

Run from the repository root:
    python3 .github/scripts/archive_app.py create path/to/Writ.app Writ_0.6.0_universal.app.tar.gz
    python3 .github/scripts/archive_app.py check Writ_0.6.0_universal.app.tar.gz Writ.app
"""
from __future__ import annotations

import argparse
import os
import pathlib
import stat
import sys
import tarfile


class ArchiveError(RuntimeError):
    pass


def bundle_entries(app: pathlib.Path) -> list[pathlib.Path]:
    entries: list[pathlib.Path] = [app]
    for root, dirs, files in os.walk(app, followlinks=False):
        dirs.sort()
        here = pathlib.Path(root)
        for name in sorted(dirs + files):
            entries.append(here / name)
    return entries


def tar_info(path: pathlib.Path, arcname: str) -> tarfile.TarInfo:
    info = tarfile.TarInfo(arcname)
    status = path.lstat()
    info.mode = stat.S_IMODE(status.st_mode)
    info.mtime = int(status.st_mtime)
    info.uid = info.gid = 0
    info.uname = info.gname = ""
    if stat.S_ISLNK(status.st_mode):
        info.type = tarfile.SYMTYPE
        info.linkname = os.readlink(path)
    elif stat.S_ISDIR(status.st_mode):
        info.type = tarfile.DIRTYPE
    elif stat.S_ISREG(status.st_mode):
        info.type = tarfile.REGTYPE
        info.size = status.st_size
    else:
        raise ArchiveError(f"{path} is neither a file, a directory nor a symlink")
    return info


def create(app: pathlib.Path, output: pathlib.Path) -> int:
    if app.suffix != ".app" or not app.is_dir() or app.is_symlink():
        raise ArchiveError(f"{app} is not an .app bundle directory")
    if not (app / "Contents" / "Info.plist").is_file():
        raise ArchiveError(f"{app} has no Contents/Info.plist")
    count = 0
    with tarfile.open(output, "w:gz", format=tarfile.PAX_FORMAT) as archive:
        for path in bundle_entries(app):
            arcname = pathlib.PurePosixPath(app.name, *path.relative_to(app).parts).as_posix()
            info = tar_info(path, arcname)
            if info.isreg():
                with path.open("rb") as handle:
                    archive.addfile(info, handle)
            else:
                archive.addfile(info)
            count += 1
    return count


def problems(archive_path: pathlib.Path, app_name: str) -> list[str]:
    found: list[str] = []
    with tarfile.open(archive_path, "r:gz") as archive:
        members = archive.getmembers()
    if not members:
        return ["the archive is empty"]
    roots = {member.name.split("/")[0] for member in members}
    if roots != {app_name}:
        found.append(f"top-level entries are {sorted(roots)}, expected only {app_name}")
    for member in members:
        parts = member.name.rstrip("/").split("/")
        if any(part in ("", ".", "..") for part in parts):
            found.append(f"{member.name} is not a plain relative path")
        if any(part.startswith("._") for part in parts):
            found.append(f"{member.name} is an AppleDouble entry")
        if member.pax_headers and any(key.startswith(("SCHILY.xattr.", "LIBARCHIVE.xattr.")) for key in member.pax_headers):
            found.append(f"{member.name} carries extended attributes")
        if not (member.isdir() or member.isreg() or member.issym()):
            found.append(f"{member.name} is a special file")
    if not any(member.name == f"{app_name}/Contents/Info.plist" for member in members):
        found.append(f"{app_name}/Contents/Info.plist is missing")
    return found


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    create_cmd = sub.add_parser("create", help="archive an .app for the updater")
    create_cmd.add_argument("app", type=pathlib.Path)
    create_cmd.add_argument("output", type=pathlib.Path)
    check_cmd = sub.add_parser("check", help="require the updater layout")
    check_cmd.add_argument("archive", type=pathlib.Path)
    check_cmd.add_argument("app_name")
    args = parser.parse_args(argv)
    try:
        if args.command == "create":
            count = create(args.app, args.output)
            found = problems(args.output, args.app.name)
            if found:
                raise ArchiveError("; ".join(found))
            print(f"wrote {args.output} with {count} entries under {args.app.name}/")
        else:
            found = problems(args.archive, args.app_name)
            if found:
                raise ArchiveError("; ".join(found))
            print(f"{args.archive} holds only {args.app_name}/")
    except (ArchiveError, OSError, tarfile.TarError) as error:
        print(f"archive_app: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
