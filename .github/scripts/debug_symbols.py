#!/usr/bin/env python3
"""Files a release build's symbol files under the build ID a crash names.

A panic report or a native crash report from a release build holds frame
addresses and the build ID of the binary they came from. The symbols that turn
those addresses back into functions and lines never ship with the app; they
are kept here, one directory per build ID, with a manifest, and the build job
uploads the directory as a CI artifact (ADR-045).

Build IDs:
  dsym  the LC_UUID of each architecture's dSYM, as `dwarfdump --uuid` prints it
  pdb   the CodeView signature in the executable: GUID then age, the key a
        symbol server files the PDB under
  elf   the GNU build ID note of the shipped (stripped) binary

Run from the repository root:
    python3 .github/scripts/debug_symbols.py dsym --out DIR A.dSYM [B.dSYM ...]
    python3 .github/scripts/debug_symbols.py pdb --out DIR --exe writ-tauri.exe writ_tauri.pdb
    python3 .github/scripts/debug_symbols.py elf --out DIR --binary writ-tauri writ-tauri.debug
"""
from __future__ import annotations

import argparse
import pathlib
import shutil
import struct
import sys

MANIFEST = "manifest.tsv"

MH_MAGIC_64 = 0xFEEDFACF
MH_MAGIC = 0xFEEDFACE
FAT_MAGIC = 0xCAFEBABE
LC_UUID = 0x1B
CPU_NAMES = {0x0100000C: "arm64", 0x01000007: "x86_64", 0x0000000C: "arm", 0x00000007: "i386"}

PT_NOTE = 4
NT_GNU_BUILD_ID = 3


class SymbolError(RuntimeError):
    pass


def macho_uuids(data: bytes) -> list[tuple[str, str]]:
    """(architecture, UUID) for each slice of a Mach-O file."""
    if len(data) < 8:
        raise SymbolError("not a Mach-O file: too short")
    if struct.unpack_from(">I", data, 0)[0] == FAT_MAGIC:
        count = struct.unpack_from(">I", data, 4)[0]
        slices = []
        for index in range(count):
            _, _, offset, size, _ = struct.unpack_from(">iiIII", data, 8 + index * 20)
            slices.extend(macho_uuids(data[offset : offset + size]))
        return slices
    magic = struct.unpack_from("<I", data, 0)[0]
    if magic not in (MH_MAGIC_64, MH_MAGIC):
        raise SymbolError(f"not a Mach-O file: magic {magic:#x}")
    cputype, _, _, ncmds, _, _ = struct.unpack_from("<iiIIII", data, 4)
    offset = 32 if magic == MH_MAGIC_64 else 28
    for _ in range(ncmds):
        cmd, cmdsize = struct.unpack_from("<II", data, offset)
        if cmd == LC_UUID:
            raw = data[offset + 8 : offset + 24]
            return [(CPU_NAMES.get(cputype & 0xFFFFFFFF, f"cpu-{cputype:#x}"), format_uuid(raw))]
        if cmdsize == 0:
            break
        offset += cmdsize
    raise SymbolError("the Mach-O file has no LC_UUID")


def format_uuid(raw: bytes) -> str:
    text = raw.hex().upper()
    return f"{text[:8]}-{text[8:12]}-{text[12:16]}-{text[16:20]}-{text[20:]}"


def pdb_key(exe: bytes, pdb_name: str) -> str:
    """The symbol-server key of the PDB the executable's CodeView record names."""
    wanted = pdb_name.lower()
    start = 0
    while (found := exe.find(b"RSDS", start)) != -1:
        start = found + 4
        if found + 24 > len(exe):
            break
        end = exe.find(b"\0", found + 24)
        if end == -1:
            break
        path = exe[found + 24 : end].decode("utf-8", "replace")
        if path.replace("\\", "/").rsplit("/", 1)[-1].lower() != wanted:
            continue
        data1, data2, data3 = struct.unpack_from("<IHH", exe, found + 4)
        data4 = exe[found + 12 : found + 20]
        age = struct.unpack_from("<I", exe, found + 20)[0]
        return f"{data1:08X}{data2:04X}{data3:04X}{data4.hex().upper()}{age:X}"
    raise SymbolError(f"the executable names no {pdb_name} in a CodeView record")


def elf_build_id(data: bytes) -> str:
    """The GNU build ID of a 64-bit little-endian ELF file, from its notes."""
    if data[:4] != b"\x7fELF":
        raise SymbolError("not an ELF file")
    if data[4] != 2 or data[5] != 1:
        raise SymbolError("only 64-bit little-endian ELF files are read")
    phoff = struct.unpack_from("<Q", data, 0x20)[0]
    phentsize, phnum = struct.unpack_from("<HH", data, 0x36)
    for index in range(phnum):
        header = phoff + index * phentsize
        p_type = struct.unpack_from("<I", data, header)[0]
        if p_type != PT_NOTE:
            continue
        p_offset = struct.unpack_from("<Q", data, header + 8)[0]
        p_filesz = struct.unpack_from("<Q", data, header + 32)[0]
        cursor, end = p_offset, p_offset + p_filesz
        while cursor + 12 <= end:
            namesz, descsz, kind = struct.unpack_from("<III", data, cursor)
            name_at = cursor + 12
            desc_at = name_at + align4(namesz)
            if kind == NT_GNU_BUILD_ID and data[name_at : name_at + namesz] == b"GNU\0":
                return data[desc_at : desc_at + descsz].hex()
            cursor = desc_at + align4(descsz)
    raise SymbolError("the ELF file has no GNU build ID note")


def align4(size: int) -> int:
    return (size + 3) & ~3


def file_under(out: pathlib.Path, build_id: str, source: pathlib.Path, label: str) -> pathlib.Path:
    target = out / build_id / source.name
    if target.exists():
        raise SymbolError(f"{target} is already filed")
    target.parent.mkdir(parents=True, exist_ok=True)
    if source.is_dir():
        shutil.copytree(source, target, symlinks=True)
    else:
        shutil.copy2(source, target)
    with (out / MANIFEST).open("a", encoding="utf-8") as manifest:
        manifest.write(f"{build_id}\t{label}\t{target.relative_to(out).as_posix()}\n")
    return target


def collect_dsym(out: pathlib.Path, dsym: pathlib.Path) -> list[pathlib.Path]:
    dwarf = dsym / "Contents" / "Resources" / "DWARF"
    files = sorted(path for path in dwarf.iterdir() if path.is_file()) if dwarf.is_dir() else []
    if len(files) != 1:
        raise SymbolError(f"{dsym} holds {len(files)} DWARF files; expected one")
    filed = []
    for arch, uuid in macho_uuids(files[0].read_bytes()):
        filed.append(file_under(out, uuid, dsym, arch))
    return filed


def collect_pdb(out: pathlib.Path, exe: pathlib.Path, pdb: pathlib.Path) -> pathlib.Path:
    return file_under(out, pdb_key(exe.read_bytes(), pdb.name), pdb, "x86_64-pc-windows-msvc")


def collect_elf(out: pathlib.Path, binary: pathlib.Path, debug: pathlib.Path) -> pathlib.Path:
    return file_under(out, elf_build_id(binary.read_bytes()), debug, "x86_64-unknown-linux-gnu")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="kind", required=True)
    dsym = commands.add_parser("dsym")
    dsym.add_argument("--out", type=pathlib.Path, required=True)
    dsym.add_argument("dsyms", type=pathlib.Path, nargs="+")
    pdb = commands.add_parser("pdb")
    pdb.add_argument("--out", type=pathlib.Path, required=True)
    pdb.add_argument("--exe", type=pathlib.Path, required=True)
    pdb.add_argument("pdb", type=pathlib.Path)
    elf = commands.add_parser("elf")
    elf.add_argument("--out", type=pathlib.Path, required=True)
    elf.add_argument("--binary", type=pathlib.Path, required=True)
    elf.add_argument("debug", type=pathlib.Path)
    args = parser.parse_args(argv)

    args.out.mkdir(parents=True, exist_ok=True)
    try:
        if args.kind == "dsym":
            for path in args.dsyms:
                collect_dsym(args.out, path)
        elif args.kind == "pdb":
            collect_pdb(args.out, args.exe, args.pdb)
        else:
            collect_elf(args.out, args.binary, args.debug)
    except (SymbolError, OSError, struct.error) as error:
        print(f"debug_symbols: {error}", file=sys.stderr)
        return 1
    print((args.out / MANIFEST).read_text(encoding="utf-8"), end="")
    return 0


if __name__ == "__main__":
    sys.exit(main())
