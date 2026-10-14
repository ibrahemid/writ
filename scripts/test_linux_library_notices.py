"""Tests for scripts/gen-linux-library-notices.py against fixture listings and packages.

Run from the repository root:
    python3 -m unittest discover -s scripts -p 'test_*.py'
"""

from __future__ import annotations

import importlib.util
import pathlib
import struct
import sys
import tempfile
import unittest

SCRIPT_PATH = pathlib.Path(__file__).with_name("gen-linux-library-notices.py")
SPEC = importlib.util.spec_from_file_location("gen_linux_library_notices", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Could not load {SCRIPT_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)

LISTING = """\
drwxr-xr-x root/root        0 2026-10-01 12:00 squashfs-root
-rwxr-xr-x root/root      100 2026-10-01 12:00 squashfs-root/AppRun
-rw-r--r-- root/root   500000 2026-10-01 12:00 squashfs-root/usr/lib/libwebkit2gtk-4.1.so.0
lrwxrwxrwx root/root       24 2026-10-01 12:00 squashfs-root/usr/lib/libwebkit2gtk-4.1.so -> libwebkit2gtk-4.1.so.0
-rw-r--r-- root/root    20000 2026-10-01 12:00 squashfs-root/usr/lib/libz.so.1
-rw-r--r-- root/root    20000 2026-10-01 12:00 squashfs-root/usr/lib/x86_64-linux-gnu/gdk-pixbuf-2.0/2.10.0/loaders/libpixbufloader-png.so
-rw-r--r-- root/root     3000 2026-10-01 12:00 squashfs-root/usr/lib/Writ/THIRD-PARTY-NOTICES.md
-rwxr-xr-x root/root  9000000 2026-10-01 12:00 squashfs-root/usr/bin/writ-tauri
"""

DEP5 = """\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: WebKitGTK

Files: Source/WTF/*
Copyright: Apple Inc.
License: BSD-2-clause

Files: *
Copyright: 2009-2024 The WebKitGTK Project
License: LGPL-2+ and BSD-2-clause

License: LGPL-2+
 On Debian systems, the complete text of the GNU Library General Public
 License version 2 can be found in `/usr/share/common-licenses/LGPL-2'.
"""

FREE_FORM = """\
This is zlib, packaged for Debian.

Copyright (C) 1995-2022 Jean-loup Gailly and Mark Adler
See /usr/share/common-licenses/LGPL-2.1. for nothing in particular.
"""


class FakeDpkg:
    def __init__(self, owners: dict[str, str]) -> None:
        self.owners = owners
        self.asked: list[str] = []

    def owner(self, path: str) -> str | None:
        self.asked.append(path)
        return self.owners.get(path)

    def source(self, package: str) -> tuple[str, str]:
        return {
            "libwebkit2gtk-4.1-0": ("webkit2gtk", "2.46.5-0ubuntu0.22.04.1"),
            "zlib1g": ("zlib", "1:1.2.11.dfsg-2ubuntu9.2"),
            "libgdk-pixbuf-2.0-0": ("gdk-pixbuf", "2.42.8+dfsg-1ubuntu0.3"),
        }[package]


OWNERS = {
    "/usr/lib/x86_64-linux-gnu/libwebkit2gtk-4.1.so.0": "libwebkit2gtk-4.1-0",
    # Recorded under /lib, as dpkg does for packages from before the /usr merge.
    "/lib/x86_64-linux-gnu/libz.so.1": "zlib1g",
    "/usr/lib/x86_64-linux-gnu/gdk-pixbuf-2.0/2.10.0/loaders/libpixbufloader-png.so": "libgdk-pixbuf-2.0-0",
}


class LinuxLibraryNoticesTests(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.doc = self.root / "doc"
        self.common = self.root / "common-licenses"
        self.common.mkdir()
        for package, text in (
            ("libwebkit2gtk-4.1-0", DEP5),
            ("zlib1g", FREE_FORM),
            ("libgdk-pixbuf-2.0-0", DEP5.replace("WebKitGTK", "gdk-pixbuf")),
        ):
            (self.doc / package).mkdir(parents=True)
            (self.doc / package / "copyright").write_text(text)
        (self.common / "LGPL-2").write_text("GNU LIBRARY GENERAL PUBLIC LICENSE\nVersion 2")
        (self.common / "LGPL-2.1").write_text("GNU LESSER GENERAL PUBLIC LICENSE\nVersion 2.1")
        (self.common / "GPL-3").write_text("not referenced")

    def section(self) -> str:
        libraries = MODULE.map_libraries(MODULE.shared_libraries(LISTING), FakeDpkg(OWNERS))
        return MODULE.render_section(libraries, self.doc, self.common)

    def test_lists_regular_library_files_under_usr_lib_only(self) -> None:
        self.assertEqual(
            MODULE.shared_libraries(LISTING),
            [
                "usr/lib/libwebkit2gtk-4.1.so.0",
                "usr/lib/libz.so.1",
                "usr/lib/x86_64-linux-gnu/gdk-pixbuf-2.0/2.10.0/loaders/libpixbufloader-png.so",
            ],
        )

    def test_finds_the_squashfs_after_the_section_headers(self) -> None:
        header = bytearray(64)
        header[:5] = b"\x7fELF\x02"
        struct.pack_into("<Q", header, 40, 1000)
        struct.pack_into("<HH", header, 58, 64, 3)
        image = self.root / "a.AppImage"
        image.write_bytes(bytes(header))
        self.assertEqual(MODULE.squashfs_offset(image), 1192)

    def test_maps_each_library_to_its_package_across_the_usr_merge(self) -> None:
        libraries = MODULE.map_libraries(MODULE.shared_libraries(LISTING), FakeDpkg(OWNERS))
        self.assertEqual(
            [(lib.path, lib.package, lib.source) for lib in libraries],
            [
                ("usr/lib/libwebkit2gtk-4.1.so.0", "libwebkit2gtk-4.1-0", "webkit2gtk"),
                ("usr/lib/libz.so.1", "zlib1g", "zlib"),
                (
                    "usr/lib/x86_64-linux-gnu/gdk-pixbuf-2.0/2.10.0/loaders/libpixbufloader-png.so",
                    "libgdk-pixbuf-2.0-0",
                    "gdk-pixbuf",
                ),
            ],
        )

    def test_a_library_no_package_owns_stops_the_release(self) -> None:
        with self.assertRaisesRegex(MODULE.NoticeError, "usr/lib/libz.so.1"):
            MODULE.map_libraries(["usr/lib/libz.so.1"], FakeDpkg({}))

    def test_reads_the_licence_debian_gives_all_files(self) -> None:
        self.assertEqual(MODULE.package_licence(DEP5), "LGPL-2+ and BSD-2-clause")
        self.assertEqual(MODULE.package_licence(FREE_FORM), "see the copyright file")

    def test_the_section_carries_the_table_the_offer_the_copyrights_and_the_texts(self) -> None:
        text = self.section()
        self.assertTrue(text.startswith(MODULE.SECTION_HEADING))
        self.assertIn(
            "| `usr/lib/libwebkit2gtk-4.1.so.0` | libwebkit2gtk-4.1-0 | webkit2gtk 2.46.5-0ubuntu0.22.04.1 "
            "| LGPL-2+ and BSD-2-clause |",
            text,
        )
        self.assertIn("For three years from the date this AppImage was published", text)
        self.assertIn("https://launchpad.net/ubuntu/+source/zlib/1:1.2.11.dfsg-2ubuntu9.2", text)
        self.assertIn("#### zlib1g", text)
        self.assertIn("Jean-loup Gailly", text)
        self.assertEqual(text.count("GNU LIBRARY GENERAL PUBLIC LICENSE"), 1)
        self.assertIn("GNU LESSER GENERAL PUBLIC LICENSE", text)
        self.assertNotIn("not referenced", text)

    def test_a_package_without_a_copyright_file_stops_the_release(self) -> None:
        (self.doc / "zlib1g" / "copyright").unlink()
        with self.assertRaisesRegex(MODULE.NoticeError, "zlib1g has no copyright file"):
            self.section()

    def test_appends_once(self) -> None:
        notices = self.root / "THIRD-PARTY-NOTICES.md"
        notices.write_text("# Third-party notices\n")
        MODULE.append_section(notices, self.section())
        self.assertIn(MODULE.SECTION_HEADING, notices.read_text())
        with self.assertRaisesRegex(MODULE.NoticeError, "already has"):
            MODULE.append_section(notices, self.section())

    def test_check_compares_what_ships_with_what_is_credited(self) -> None:
        shipped = MODULE.shared_libraries(LISTING)
        credited = MODULE.credited_libraries("# Notices\n\n" + self.section())
        self.assertEqual(MODULE.mismatch(shipped, credited), [])
        self.assertEqual(
            MODULE.mismatch(shipped + ["usr/lib/libnew.so.1"], credited[1:]),
            ["not credited: usr/lib/libnew.so.1", f"not credited: {shipped[0]}"],
        )
        self.assertEqual(MODULE.credited_libraries("# Notices\n"), [])


if __name__ == "__main__":
    unittest.main()
