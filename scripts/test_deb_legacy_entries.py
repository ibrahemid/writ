"""Tests for scripts/deb_legacy_entries.py over debs laid out as the Tauri bundler writes them.

Run from the repository root:
    python3 -m unittest discover -s scripts -p 'test_*.py'
"""

from __future__ import annotations

import gzip
import hashlib
import importlib.util
import io
import lzma
import pathlib
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).with_name("deb_legacy_entries.py")
SPEC = importlib.util.spec_from_file_location("deb_legacy_entries", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Could not load {SCRIPT}")
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)

MTIME = 1793082369
CLI = b"\x7fELF command build" * 200
GUI = b"\x7fELF gui build" * 300
DESKTOP = (
    "[Desktop Entry]\n"
    "Categories=Office;\n"
    "Comment=Light text editor\n"
    "Exec=jotra-tauri\n"
    "StartupWMClass=jotra-tauri\n"
    "Icon=jotra-tauri\n"
    "Name=Jotra\n"
    "Terminal=false\n"
    "Type=Application\n"
    "MimeType=text/plain;text/markdown;x-scheme-handler/jotra;x-scheme-handler/writ\n"
)
DATA_LAYOUT: list[tuple[str, bytes | None, int]] = [
    ("usr", None, 0o755),
    ("usr/bin", None, 0o755),
    ("usr/bin/jotra", CLI, 0o755),
    ("usr/bin/jotra-tauri", GUI, 0o755),
    ("usr/share", None, 0o755),
    ("usr/share/applications", None, 0o755),
    ("usr/share/applications/Jotra.desktop", DESKTOP.encode(), 0o644),
]


def build_tar(entries: list[tuple[str, bytes | None, int]]) -> bytes:
    out = io.BytesIO()
    with tarfile.open(fileobj=out, mode="w", format=tarfile.GNU_FORMAT) as archive:
        for name, data, mode in entries:
            info = tarfile.TarInfo(name)
            info.mode = mode
            info.mtime = MTIME
            if data is None:
                info.type = tarfile.DIRTYPE
                archive.addfile(info)
            else:
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))
    return out.getvalue()


def build_ar(members: list[tuple[str, bytes]]) -> bytes:
    out = bytearray(b"!<arch>\n")
    for name, data in members:
        out += f"{name:<16}{MTIME:<12}{1001:<6}{1001:<6}{'100644':<8}{len(data):<10}`\n".encode()
        out += data
        if len(data) % 2:
            out += b"\n"
    return bytes(out)


def build_deb(data_entries: list[tuple[str, bytes | None, int]] = DATA_LAYOUT, compression: str = "gz") -> bytes:
    files = [(name, data) for name, data, _ in data_entries if data is not None]
    md5sums = "".join(f"{hashlib.md5(data).hexdigest()}  {name}\n" for name, data in files)
    control = (
        "Package: jotra\nVersion: 1.1.0\nArchitecture: amd64\nInstalled-Size: 100\n"
        "Maintainer: Ibrahem ID\nConflicts: writ\nReplaces: writ\nProvides: writ\n"
        "Description: Light text editor\n"
    )
    control_tar = build_tar([("control", control.encode(), 0o644), ("md5sums", md5sums.encode(), 0o644)])
    data_tar = build_tar(data_entries)
    compressed = gzip.compress(data_tar) if compression == "gz" else lzma.compress(data_tar)
    return build_ar(
        [
            ("debian-binary", b"2.0\n"),
            ("control.tar.gz", gzip.compress(control_tar)),
            (f"data.tar.{compression}", compressed),
        ]
    )


def read_tar(deb: bytes, prefix: str) -> dict[str, tuple[tarfile.TarInfo, bytes | None]]:
    member = next(m for m in MODULE.read_ar_members(deb) if m.name.startswith(prefix))
    entries: dict[str, tuple[tarfile.TarInfo, bytes | None]] = {}
    with tarfile.open(fileobj=io.BytesIO(MODULE.decompress_tar(member)), mode="r:") as archive:
        for info in archive.getmembers():
            extracted = archive.extractfile(info) if info.isreg() else None
            entries[info.name] = (info, extracted.read() if extracted is not None else None)
    return entries


def read_desktop_keys(text: str) -> dict[str, str]:
    return dict(line.split("=", 1) for line in text.splitlines() if "=" in line)


class AddLegacyEntriesTests(unittest.TestCase):
    def test_the_launcher_link_points_at_the_new_gui_binary(self) -> None:
        deb, added = MODULE.add_legacy_entries(build_deb())
        info, _ = read_tar(deb, "data.tar")["usr/bin/writ-tauri"]
        self.assertTrue(info.issym())
        self.assertEqual(info.linkname, "jotra-tauri")
        self.assertIn("/usr/bin/writ-tauri", added)

    def test_the_legacy_command_is_a_copy_of_the_command(self) -> None:
        deb, _ = MODULE.add_legacy_entries(build_deb())
        entries = read_tar(deb, "data.tar")
        info, data = entries["usr/bin/writ"]
        self.assertTrue(info.isreg())
        self.assertEqual(data, CLI)
        self.assertEqual(info.mode, entries["usr/bin/jotra"][0].mode)

    def test_a_shipped_legacy_command_is_kept(self) -> None:
        layout = [*DATA_LAYOUT[:3], ("usr/bin/writ", b"shipped", 0o755), *DATA_LAYOUT[3:]]
        deb, added = MODULE.add_legacy_entries(build_deb(layout))
        self.assertEqual(read_tar(deb, "data.tar")["usr/bin/writ"][1], b"shipped")
        self.assertNotIn("/usr/bin/writ", added)

    def test_the_legacy_desktop_entry_is_hidden_with_the_same_exec_and_types(self) -> None:
        deb, _ = MODULE.add_legacy_entries(build_deb())
        _, data = read_tar(deb, "data.tar")["usr/share/applications/Writ.desktop"]
        assert data is not None
        legacy = read_desktop_keys(data.decode())
        current = read_desktop_keys(DESKTOP)
        self.assertEqual(legacy["NoDisplay"], "true")
        self.assertEqual(legacy["Exec"], current["Exec"])
        self.assertEqual(legacy["MimeType"], current["MimeType"])
        self.assertEqual({**legacy, "NoDisplay": None}, {**current, "NoDisplay": None})

    def test_an_existing_display_key_is_replaced(self) -> None:
        text = MODULE.build_legacy_desktop_entry("[Desktop Entry]\nNoDisplay=false\nExec=x\n\n[Desktop Action new]\nExec=y\n")
        self.assertEqual(text, "[Desktop Entry]\nExec=x\nNoDisplay=true\n\n[Desktop Action new]\nExec=y\n")

    def test_entries_sit_after_their_sources_and_the_rest_is_unchanged(self) -> None:
        deb, _ = MODULE.add_legacy_entries(build_deb())
        self.assertEqual(
            list(read_tar(deb, "data.tar")),
            [
                "usr",
                "usr/bin",
                "usr/bin/jotra",
                "usr/bin/writ",
                "usr/bin/jotra-tauri",
                "usr/bin/writ-tauri",
                "usr/share",
                "usr/share/applications",
                "usr/share/applications/Jotra.desktop",
                "usr/share/applications/Writ.desktop",
            ],
        )
        original = MODULE.read_ar_members(build_deb())
        rewritten = MODULE.read_ar_members(deb)
        self.assertEqual([m.name for m in rewritten], ["debian-binary", "control.tar.gz", "data.tar.gz"])
        self.assertEqual(rewritten[0], original[0])
        self.assertEqual([m.header[:48] for m in rewritten], [m.header[:48] for m in original])

    def test_md5sums_and_installed_size_cover_the_added_files(self) -> None:
        deb, _ = MODULE.add_legacy_entries(build_deb())
        control = read_tar(deb, "control.tar")
        sums = (control["md5sums"][1] or b"").decode().splitlines()
        desktop = read_tar(deb, "data.tar")["usr/share/applications/Writ.desktop"][1] or b""
        self.assertIn(f"{hashlib.md5(CLI).hexdigest()}  usr/bin/writ", sums)
        self.assertIn(f"{hashlib.md5(desktop).hexdigest()}  usr/share/applications/Writ.desktop", sums)
        self.assertFalse(any(line.endswith("writ-tauri") for line in sums))
        expected = 100 + -(-len(CLI) // 1024) + -(-len(desktop) // 1024) + 1
        self.assertIn(f"Installed-Size: {expected}\n", (control["control"][1] or b"").decode())
        self.assertIn("Conflicts: writ\n", (control["control"][1] or b"").decode())

    def test_an_xz_data_archive_is_written_back_as_xz(self) -> None:
        deb, _ = MODULE.add_legacy_entries(build_deb(compression="xz"))
        member = MODULE.read_ar_members(deb)[2]
        self.assertEqual(member.name, "data.tar.xz")
        self.assertTrue(member.data.startswith(b"\xfd7zXZ\x00"))
        self.assertIn("usr/bin/writ-tauri", read_tar(deb, "data.tar"))

    def test_a_deb_that_already_holds_the_link_is_refused(self) -> None:
        layout = [*DATA_LAYOUT, ("usr/bin/writ-tauri", b"gui", 0o755)]
        with self.assertRaisesRegex(MODULE.DebEntryError, "already holds /usr/bin/writ-tauri"):
            MODULE.add_legacy_entries(build_deb(layout))

    def test_a_deb_without_the_gui_binary_is_refused(self) -> None:
        layout = [entry for entry in DATA_LAYOUT if entry[0] != "usr/bin/jotra-tauri"]
        with self.assertRaisesRegex(MODULE.DebEntryError, "no /usr/bin/jotra-tauri"):
            MODULE.add_legacy_entries(build_deb(layout))

    def test_an_unsupported_compression_is_refused(self) -> None:
        deb = build_ar([("debian-binary", b"2.0\n"), ("control.tar.gz", b""), ("data.tar.zst", b"zstd")])
        with self.assertRaisesRegex(MODULE.DebFormatError, "unsupported compression in data.tar.zst"):
            MODULE.add_legacy_entries(deb)

    def test_a_file_that_is_not_an_ar_archive_is_refused(self) -> None:
        with self.assertRaisesRegex(MODULE.DebFormatError, "not an ar archive"):
            MODULE.add_legacy_entries(b"PK\x03\x04")


class RewriteDebTests(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="jotra-deb-test-")
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.deb = self.root / "Jotra_1.1.0_amd64.deb"
        self.deb.write_bytes(build_deb())
        self.deb.chmod(0o644)

    def run_script(self) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), str(self.deb)], capture_output=True, text=True, check=False
        )

    def test_the_deb_is_rewritten_in_place(self) -> None:
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("usr/bin/writ-tauri", read_tar(self.deb.read_bytes(), "data.tar"))
        self.assertEqual(self.deb.stat().st_mode & 0o777, 0o644)
        self.assertEqual(sorted(path.name for path in self.root.iterdir()), [self.deb.name])

    def test_a_second_run_fails_and_leaves_the_deb_as_it_was(self) -> None:
        self.assertEqual(self.run_script().returncode, 0)
        before = self.deb.read_bytes()
        result = self.run_script()
        self.assertEqual(result.returncode, 1)
        self.assertIn("already holds /usr/bin/writ-tauri", result.stderr)
        self.assertEqual(self.deb.read_bytes(), before)

    @unittest.skipUnless(shutil.which("dpkg-deb"), "dpkg-deb is not installed")
    def test_dpkg_reads_the_rewritten_deb(self) -> None:
        self.assertEqual(self.run_script().returncode, 0)
        listing = subprocess.run(["dpkg-deb", "-c", str(self.deb)], capture_output=True, text=True, check=True).stdout
        self.assertRegex(listing, r"usr/bin/writ-tauri -> jotra-tauri")
        self.assertRegex(listing, r" \./usr/bin/writ\n|usr/bin/writ\n")
        self.assertIn("usr/share/applications/Writ.desktop", listing)
        fields = subprocess.run(
            ["dpkg-deb", "-f", str(self.deb), "Replaces"], capture_output=True, text=True, check=True
        ).stdout
        self.assertEqual(fields.strip(), "writ")


if __name__ == "__main__":
    unittest.main()
