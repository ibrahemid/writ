"""Tests for debug_symbols.py.

Run from the repository root:
    python3 -m unittest discover -s .github/scripts -t .github/scripts
"""

from __future__ import annotations

import contextlib
import io
import pathlib
import struct
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import debug_symbols  # noqa: E402

ARM64 = 0x0100000C
X86_64 = 0x01000007
UUID_A = bytes(range(16))
UUID_B = bytes(range(16, 32))


def thin_macho(cputype: int, uuid: bytes) -> bytes:
    segment = struct.pack("<II", 0x19, 16) + b"\0" * 8
    uuid_command = struct.pack("<II", debug_symbols.LC_UUID, 24) + uuid
    commands = segment + uuid_command
    header = struct.pack("<IiiIIIII", debug_symbols.MH_MAGIC_64, cputype, 0, 0xA, 2, len(commands), 0, 0)
    return header + commands


def fat_macho(*slices: tuple[int, bytes]) -> bytes:
    bodies = [thin_macho(cputype, uuid) for cputype, uuid in slices]
    offset = 8 + 20 * len(bodies)
    header = struct.pack(">II", debug_symbols.FAT_MAGIC, len(bodies))
    for (cputype, _), body in zip(slices, bodies):
        header += struct.pack(">iiIII", cputype, 0, offset, len(body), 0)
        offset += len(body)
    return header + b"".join(bodies)


def elf_with_build_id(build_id: bytes | None) -> bytes:
    notes = b""
    if build_id is not None:
        notes = struct.pack("<III", 4, len(build_id), debug_symbols.NT_GNU_BUILD_ID) + b"GNU\0" + build_id
    phoff = 64
    note_offset = phoff + 56
    ident = b"\x7fELF" + bytes([2, 1, 1]) + b"\0" * 9
    header = ident + struct.pack("<HHIQQQIHHHHHH", 3, 62, 1, 0, phoff, 0, 0, 64, 56, 1, 64, 0, 0)
    program = struct.pack("<IIQQQQQQ", debug_symbols.PT_NOTE, 4, note_offset, 0, 0, len(notes), len(notes), 4)
    return header + program + notes


def exe_with_codeview(pdb_path: str, guid: bytes, age: int) -> bytes:
    decoy = b"RSDS" + b"\x11" * 16 + struct.pack("<I", 9) + b"C:\\other\\vendor.pdb\0"
    record = b"RSDS" + guid + struct.pack("<I", age) + pdb_path.encode() + b"\0"
    return b"MZ" + b"\0" * 62 + decoy + b"\0" * 32 + record + b"\0" * 16


def write_dsym(root: pathlib.Path, dwarf: bytes) -> pathlib.Path:
    dsym = root / "writ-tauri.dSYM"
    folder = dsym / "Contents" / "Resources" / "DWARF"
    folder.mkdir(parents=True)
    (folder / "writ-tauri").write_bytes(dwarf)
    (dsym / "Contents" / "Info.plist").write_text("<plist/>")
    return dsym


class MachOTests(unittest.TestCase):
    def test_a_thin_file_yields_its_architecture_and_uuid(self) -> None:
        self.assertEqual(
            debug_symbols.macho_uuids(thin_macho(ARM64, UUID_A)),
            [("arm64", "00010203-0405-0607-0809-0A0B0C0D0E0F")],
        )

    def test_a_fat_file_yields_every_slice(self) -> None:
        self.assertEqual(
            [arch for arch, _ in debug_symbols.macho_uuids(fat_macho((ARM64, UUID_A), (X86_64, UUID_B)))],
            ["arm64", "x86_64"],
        )

    def test_a_file_without_lc_uuid_is_refused(self) -> None:
        header = struct.pack("<IiiIIIII", debug_symbols.MH_MAGIC_64, ARM64, 0, 0xA, 0, 0, 0, 0)
        with self.assertRaises(debug_symbols.SymbolError):
            debug_symbols.macho_uuids(header)

    def test_anything_else_is_refused(self) -> None:
        with self.assertRaises(debug_symbols.SymbolError):
            debug_symbols.macho_uuids(b"\x7fELF" + b"\0" * 60)


class CodeViewTests(unittest.TestCase):
    GUID = struct.pack("<IHH", 0x12345678, 0x9ABC, 0xDEF0) + bytes.fromhex("0102030405060708")

    def test_the_key_is_guid_then_age_for_the_named_pdb(self) -> None:
        exe = exe_with_codeview("D:\\a\\writ\\target\\release\\deps\\writ_tauri.pdb", self.GUID, 3)
        self.assertEqual(debug_symbols.pdb_key(exe, "writ_tauri.pdb"), "123456789ABCDEF001020304050607083")

    def test_an_executable_naming_another_pdb_is_refused(self) -> None:
        exe = exe_with_codeview("D:\\a\\other.pdb", self.GUID, 1)
        with self.assertRaises(debug_symbols.SymbolError):
            debug_symbols.pdb_key(exe, "writ_tauri.pdb")


class ElfTests(unittest.TestCase):
    def test_the_build_id_note_is_read(self) -> None:
        build_id = bytes.fromhex("8f3a" * 10)
        self.assertEqual(debug_symbols.elf_build_id(elf_with_build_id(build_id)), build_id.hex())

    def test_a_binary_without_a_build_id_is_refused(self) -> None:
        with self.assertRaises(debug_symbols.SymbolError):
            debug_symbols.elf_build_id(elf_with_build_id(None))


class CollectTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.out = self.root / "debug-symbols-macos"

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def run_main(self, *argv: str) -> int:
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return debug_symbols.main(list(argv))

    def manifest(self) -> list[list[str]]:
        text = (self.out / debug_symbols.MANIFEST).read_text(encoding="utf-8")
        return [line.split("\t") for line in text.splitlines()]

    def test_each_dsym_is_filed_under_its_uuid(self) -> None:
        arm = write_dsym(self.root / "arm", thin_macho(ARM64, UUID_A))
        intel = write_dsym(self.root / "intel", thin_macho(X86_64, UUID_B))

        self.assertEqual(self.run_main("dsym", "--out", str(self.out), str(arm), str(intel)), 0)

        uuid_a = debug_symbols.format_uuid(UUID_A)
        self.assertTrue((self.out / uuid_a / "writ-tauri.dSYM" / "Contents" / "Resources" / "DWARF" / "writ-tauri").is_file())
        self.assertEqual(
            self.manifest(),
            [
                [uuid_a, "arm64", f"{uuid_a}/writ-tauri.dSYM"],
                [debug_symbols.format_uuid(UUID_B), "x86_64", f"{debug_symbols.format_uuid(UUID_B)}/writ-tauri.dSYM"],
            ],
        )

    def test_the_pdb_is_filed_under_the_executables_key(self) -> None:
        exe = self.root / "writ-tauri.exe"
        exe.write_bytes(exe_with_codeview("D:\\a\\writ_tauri.pdb", CodeViewTests.GUID, 1))
        pdb = self.root / "writ_tauri.pdb"
        pdb.write_bytes(b"Microsoft C/C++ MSF 7.00\r\n")

        self.assertEqual(self.run_main("pdb", "--out", str(self.out), "--exe", str(exe), str(pdb)), 0)

        self.assertEqual((self.out / "123456789ABCDEF001020304050607081" / "writ_tauri.pdb").read_bytes(), pdb.read_bytes())

    def test_the_debug_file_is_filed_under_the_shipped_binarys_build_id(self) -> None:
        binary = self.root / "writ-tauri"
        binary.write_bytes(elf_with_build_id(bytes.fromhex("ab" * 20)))
        debug = self.root / "writ-tauri.debug"
        debug.write_bytes(b"\x7fELF debug")

        self.assertEqual(self.run_main("elf", "--out", str(self.out), "--binary", str(binary), str(debug)), 0)

        self.assertTrue((self.out / ("ab" * 20) / "writ-tauri.debug").is_file())

    def test_a_dsym_without_its_dwarf_file_fails_the_step(self) -> None:
        empty = self.root / "writ-tauri.dSYM"
        empty.mkdir()
        self.assertEqual(self.run_main("dsym", "--out", str(self.out), str(empty)), 1)

    def test_the_same_build_filed_twice_fails_the_step(self) -> None:
        arm = write_dsym(self.root / "arm", thin_macho(ARM64, UUID_A))
        self.assertEqual(self.run_main("dsym", "--out", str(self.out), str(arm)), 0)
        self.assertEqual(self.run_main("dsym", "--out", str(self.out), str(arm)), 1)


if __name__ == "__main__":
    unittest.main()
