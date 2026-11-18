#!/usr/bin/env python3
"""Add the legacy command, launcher link and desktop entry to a built deb.

The deb bundler copies through symlinks, so the package-owned launcher link
cannot come from the bundle config; this rewrites the deb's data archive.

Usage:
    python3 scripts/deb_legacy_entries.py <path/to/Jotra_<version>_amd64.deb>
"""

from __future__ import annotations

import argparse
import copy
import dataclasses
import gzip
import hashlib
import io
import lzma
import math
import os
import pathlib
import re
import sys
import tarfile
import tempfile
import zlib
from typing import Sequence

PRODUCT_NAME = "Jotra"
LEGACY_NAME = "Writ"

COMMAND = PRODUCT_NAME.lower()
LEGACY_COMMAND = LEGACY_NAME.lower()
GUI = f"{COMMAND}-tauri"
LEGACY_GUI = f"{LEGACY_COMMAND}-tauri"
BIN_DIR = "usr/bin"
APPLICATIONS_DIR = "usr/share/applications"
GUI_PATH = f"{BIN_DIR}/{GUI}"
COMMAND_PATH = f"{BIN_DIR}/{COMMAND}"
DESKTOP_PATH = f"{APPLICATIONS_DIR}/{PRODUCT_NAME}.desktop"
LEGACY_GUI_PATH = f"{BIN_DIR}/{LEGACY_GUI}"
LEGACY_COMMAND_PATH = f"{BIN_DIR}/{LEGACY_COMMAND}"
LEGACY_DESKTOP_PATH = f"{APPLICATIONS_DIR}/{LEGACY_NAME}.desktop"

AR_MAGIC = b"!<arch>\n"
AR_HEADER_SIZE = 60
AR_FILE_MAGIC = b"`\n"
DESKTOP_GROUP = "[Desktop Entry]"


class DebFormatError(ValueError):
    """The file is not a deb this script can read and write back."""


class DebEntryError(ValueError):
    """The deb lacks an entry the legacy entries derive from, or already holds one of them."""


@dataclasses.dataclass(frozen=True)
class ArMember:
    header: bytes
    data: bytes

    @property
    def name(self) -> str:
        return self.header[:16].decode("ascii").rstrip(" ").rstrip("/")


@dataclasses.dataclass(frozen=True)
class TarMember:
    info: tarfile.TarInfo
    data: bytes | None

    @property
    def path(self) -> str:
        name = self.info.name[2:] if self.info.name.startswith("./") else self.info.name
        return name.rstrip("/")

    def read_data(self) -> bytes:
        if self.data is None:
            raise DebEntryError(f"/{self.path} in the deb is not a regular file")
        return self.data


def read_ar_members(deb: bytes) -> list[ArMember]:
    if not deb.startswith(AR_MAGIC):
        raise DebFormatError("not an ar archive")
    members: list[ArMember] = []
    offset = len(AR_MAGIC)
    while offset < len(deb):
        header = deb[offset : offset + AR_HEADER_SIZE]
        if len(header) != AR_HEADER_SIZE or header[58:60] != AR_FILE_MAGIC:
            raise DebFormatError(f"bad ar member header at byte {offset}")
        try:
            size = int(header[48:58].decode("ascii").strip())
        except ValueError as error:
            raise DebFormatError(f"bad ar member size at byte {offset}") from error
        start = offset + AR_HEADER_SIZE
        if start + size > len(deb):
            raise DebFormatError(f"ar member at byte {offset} runs past the end of the file")
        members.append(ArMember(header=header, data=deb[start : start + size]))
        offset = start + size + size % 2
    return members


def write_ar_members(members: Sequence[ArMember]) -> bytes:
    out = bytearray(AR_MAGIC)
    for member in members:
        size = str(len(member.data)).encode("ascii")
        if len(size) > 10:
            raise DebFormatError(f"ar member {member.name} is too large")
        out += member.header[:48] + size.ljust(10) + AR_FILE_MAGIC
        out += member.data
        if len(member.data) % 2:
            out += b"\n"
    return bytes(out)


def get_ar_member_index(members: Sequence[ArMember], prefix: str) -> int:
    found = [index for index, member in enumerate(members) if member.name.startswith(prefix)]
    if len(found) != 1:
        raise DebFormatError(f"expected one {prefix}* member, found {len(found)}")
    return found[0]


def decompress_tar(member: ArMember) -> bytes:
    if member.name.endswith(".tar"):
        return member.data
    if member.name.endswith(".tar.gz"):
        return gzip.decompress(member.data)
    if member.name.endswith(".tar.xz"):
        return lzma.decompress(member.data)
    raise DebFormatError(f"unsupported compression in {member.name}")


def compress_tar(member: ArMember, tar: bytes) -> ArMember:
    if member.name.endswith(".tar"):
        data = tar
    elif member.name.endswith(".tar.gz"):
        data = gzip.compress(tar, compresslevel=9, mtime=0)
    elif member.name.endswith(".tar.xz"):
        data = lzma.compress(tar, format=lzma.FORMAT_XZ, check=lzma.CHECK_CRC64)
    else:
        raise DebFormatError(f"unsupported compression in {member.name}")
    return ArMember(header=member.header, data=data)


def read_tar_members(tar: bytes) -> list[TarMember]:
    members: list[TarMember] = []
    with tarfile.open(fileobj=io.BytesIO(tar), mode="r:") as archive:
        for info in archive.getmembers():
            data = None
            if info.isreg():
                extracted = archive.extractfile(info)
                if extracted is None:
                    raise DebFormatError(f"cannot read {info.name}")
                data = extracted.read()
            members.append(TarMember(info=info, data=data))
    return members


def write_tar_members(members: Sequence[TarMember]) -> bytes:
    out = io.BytesIO()
    with tarfile.open(fileobj=out, mode="w", format=tarfile.GNU_FORMAT) as archive:
        for member in members:
            archive.addfile(member.info, io.BytesIO(member.data) if member.data is not None else None)
    return out.getvalue()


def find_tar_member(members: Sequence[TarMember], path: str) -> int | None:
    return next((index for index, member in enumerate(members) if member.path == path), None)


def get_regular_member(members: Sequence[TarMember], path: str) -> TarMember:
    index = find_tar_member(members, path)
    if index is None:
        raise DebEntryError(f"the deb has no /{path}")
    member = members[index]
    member.read_data()
    return member


def derive_member(source: TarMember, path: str, *, data: bytes | None = None, link: str | None = None) -> TarMember:
    info = copy.copy(source.info)
    info.name = ("./" if source.info.name.startswith("./") else "") + path
    if link is not None:
        info.type = tarfile.SYMTYPE
        info.linkname = link
        info.size = 0
        info.mode = 0o777
        return TarMember(info=info, data=None)
    if data is None:
        raise ValueError("a derived regular file needs its data")
    info.size = len(data)
    return TarMember(info=info, data=data)


def replace_member_data(member: TarMember, data: bytes) -> TarMember:
    info = copy.copy(member.info)
    info.size = len(data)
    return TarMember(info=info, data=data)


def build_legacy_desktop_entry(text: str) -> str:
    lines = text.split("\n")
    try:
        start = lines.index(DESKTOP_GROUP)
    except ValueError as error:
        raise DebEntryError(f"/{DESKTOP_PATH} has no {DESKTOP_GROUP} group") from error
    end = next((index for index in range(start + 1, len(lines)) if lines[index].startswith("[")), len(lines))
    body = [line for line in lines[start + 1 : end] if not re.match(r"\s*NoDisplay\s*=", line)]
    insert_at = len(body)
    while insert_at > 0 and body[insert_at - 1] == "":
        insert_at -= 1
    body.insert(insert_at, "NoDisplay=true")
    return "\n".join([*lines[: start + 1], *body, *lines[end:]])


def add_legacy_data_members(members: Sequence[TarMember]) -> tuple[list[TarMember], list[TarMember]]:
    gui = get_regular_member(members, GUI_PATH)
    command = get_regular_member(members, COMMAND_PATH)
    desktop = get_regular_member(members, DESKTOP_PATH)
    for path in (LEGACY_GUI_PATH, LEGACY_DESKTOP_PATH):
        if find_tar_member(members, path) is not None:
            raise DebEntryError(f"the deb already holds /{path}")
    try:
        desktop_text = desktop.read_data().decode("utf-8")
    except UnicodeDecodeError as error:
        raise DebEntryError(f"/{DESKTOP_PATH} is not UTF-8") from error

    placements = [(GUI_PATH, derive_member(gui, LEGACY_GUI_PATH, link=GUI))]
    if find_tar_member(members, LEGACY_COMMAND_PATH) is None:
        placements.append((COMMAND_PATH, derive_member(command, LEGACY_COMMAND_PATH, data=command.read_data())))
    legacy_desktop = build_legacy_desktop_entry(desktop_text).encode("utf-8")
    placements.append((DESKTOP_PATH, derive_member(desktop, LEGACY_DESKTOP_PATH, data=legacy_desktop)))

    result = list(members)
    for after, member in placements:
        index = find_tar_member(result, after)
        if index is None:
            raise DebEntryError(f"the deb has no /{after}")
        result.insert(index + 1, member)
    return result, [member for _, member in placements]


def update_control_members(members: Sequence[TarMember], added: Sequence[TarMember]) -> list[TarMember]:
    result = list(members)
    control_index = find_tar_member(result, "control")
    if control_index is None:
        raise DebFormatError("control.tar has no control file")
    control = result[control_index].read_data().decode("utf-8")
    match = re.search(r"^Installed-Size: (\d+)$", control, flags=re.MULTILINE)
    if match is None:
        raise DebFormatError("control has no Installed-Size field")
    extra = sum(math.ceil(len(member.data) / 1024) if member.data is not None else 1 for member in added)
    control = f"{control[: match.start(1)]}{int(match.group(1)) + extra}{control[match.end(1) :]}"
    result[control_index] = replace_member_data(result[control_index], control.encode("utf-8"))

    sums_index = find_tar_member(result, "md5sums")
    if sums_index is not None:
        sums = result[sums_index].read_data().decode("utf-8")
        if sums and not sums.endswith("\n"):
            sums += "\n"
        for member in added:
            if member.data is not None:
                sums += f"{hashlib.md5(member.data).hexdigest()}  {member.path}\n"
        result[sums_index] = replace_member_data(result[sums_index], sums.encode("utf-8"))
    return result


def add_legacy_entries(deb: bytes) -> tuple[bytes, list[str]]:
    ar_members = read_ar_members(deb)
    data_index = get_ar_member_index(ar_members, "data.tar")
    control_index = get_ar_member_index(ar_members, "control.tar")

    data_members, added = add_legacy_data_members(read_tar_members(decompress_tar(ar_members[data_index])))
    control_members = update_control_members(read_tar_members(decompress_tar(ar_members[control_index])), added)

    ar_members[data_index] = compress_tar(ar_members[data_index], write_tar_members(data_members))
    ar_members[control_index] = compress_tar(ar_members[control_index], write_tar_members(control_members))
    return write_ar_members(ar_members), [f"/{member.path}" for member in added]


def rewrite_deb(path: pathlib.Path) -> list[str]:
    rewritten, added = add_legacy_entries(path.read_bytes())
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix=f".{path.name}.", delete=False) as handle:
        handle.write(rewritten)
        temporary = pathlib.Path(handle.name)
    try:
        os.chmod(temporary, path.stat().st_mode & 0o7777)
        os.replace(temporary, path)
    except OSError:
        temporary.unlink(missing_ok=True)
        raise
    return added


def parse_args(argv: Sequence[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("deb", type=pathlib.Path)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        added = rewrite_deb(args.deb)
    except (DebFormatError, DebEntryError, OSError, tarfile.TarError, lzma.LZMAError, zlib.error, EOFError) as error:
        print(f"deb_legacy_entries: {args.deb}: {error}", file=sys.stderr)
        return 1
    print(f"{args.deb}: added {', '.join(added)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
