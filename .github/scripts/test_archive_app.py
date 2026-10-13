"""Tests for archive_app.py.

Run from the repository root:
    python3 -m unittest discover -s .github/scripts -t .github/scripts
"""

from __future__ import annotations

import contextlib
import io
import os
import pathlib
import platform
import plistlib
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import archive_app  # noqa: E402

ON_MAC = platform.system() == "Darwin" and shutil.which("codesign") is not None


def write_app(root: pathlib.Path, name: str = "Writ.app") -> pathlib.Path:
    app = root / name
    (app / "Contents" / "MacOS").mkdir(parents=True)
    (app / "Contents" / "Resources").mkdir()
    with (app / "Contents" / "Info.plist").open("wb") as handle:
        plistlib.dump({"CFBundleExecutable": "writ-tauri", "CFBundleIdentifier": "com.writ.editor"}, handle)
    for name in ("writ-tauri", "writ"):
        binary = app / "Contents" / "MacOS" / name
        binary.write_bytes(f"binary {name}".encode())
        binary.chmod(0o755)
    notices = app / "Contents" / "Resources" / "THIRD-PARTY-NOTICES.md"
    notices.write_text("notices\n")
    notices.chmod(0o644)
    return app


def install_like_the_updater(archive_path: pathlib.Path, destination: pathlib.Path) -> None:
    """tauri-plugin-updater drops each entry's first component and unpacks the rest."""
    destination.mkdir()
    with tarfile.open(archive_path, "r:gz") as archive:
        for member in archive.getmembers():
            parts = member.name.split("/")[1:]
            if not parts or parts == [""]:
                continue
            target = destination.joinpath(*parts)
            target.parent.mkdir(parents=True, exist_ok=True)
            if member.isdir():
                target.mkdir(exist_ok=True)
            elif member.issym():
                target.symlink_to(member.linkname)
            else:
                target.write_bytes(archive.extractfile(member).read())
            if not member.issym():
                target.chmod(member.mode)


def snapshot(root: pathlib.Path) -> dict[str, tuple[str, int, bytes | str]]:
    entries = {}
    for path in sorted(root.rglob("*")):
        relative = str(path.relative_to(root))
        mode = stat.S_IMODE(path.lstat().st_mode)
        if path.is_symlink():
            entries[relative] = ("link", mode, os.readlink(path))
        elif path.is_dir():
            entries[relative] = ("dir", mode, b"")
        else:
            entries[relative] = ("file", mode, path.read_bytes())
    return entries


def tar_with(path: pathlib.Path, names: list[str]) -> None:
    with tarfile.open(path, "w:gz") as archive:
        for name in names:
            info = tarfile.TarInfo(name)
            if name.endswith("/"):
                info.type = tarfile.DIRTYPE
                archive.addfile(info)
            else:
                data = b"x"
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))


class ArchiveAppTests(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="writ-archive-test-")
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)

    def test_the_archive_holds_only_the_app_with_modes_kept(self) -> None:
        app = write_app(self.root)
        output = self.root / "Writ_0.6.0_universal.app.tar.gz"
        archive_app.create(app, output)

        with tarfile.open(output, "r:gz") as archive:
            members = {member.name: member for member in archive.getmembers()}
        self.assertEqual(members["Writ.app"].type, tarfile.DIRTYPE)
        self.assertTrue(all(name == "Writ.app" or name.startswith("Writ.app/") for name in members))
        self.assertEqual(stat.S_IMODE(members["Writ.app/Contents/MacOS/writ-tauri"].mode), 0o755)
        self.assertEqual(stat.S_IMODE(members["Writ.app/Contents/Resources/THIRD-PARTY-NOTICES.md"].mode), 0o644)
        self.assertEqual(archive_app.problems(output, "Writ.app"), [])

    def test_the_updater_unpacks_it_into_the_same_bundle(self) -> None:
        app = write_app(self.root)
        (app / "Contents" / "Frameworks").mkdir()
        (app / "Contents" / "Frameworks" / "Current").symlink_to("A")
        output = self.root / "Writ.app.tar.gz"
        archive_app.create(app, output)
        installed = self.root / "installed"
        install_like_the_updater(output, installed)
        self.assertEqual(snapshot(installed), snapshot(app))

    def test_entries_are_ordered_parents_first(self) -> None:
        app = write_app(self.root)
        output = self.root / "Writ.app.tar.gz"
        archive_app.create(app, output)
        with tarfile.open(output, "r:gz") as archive:
            names = archive.getnames()
        for index, name in enumerate(names):
            parent = name.rsplit("/", 1)[0]
            if "/" in name:
                self.assertIn(parent, names[:index], f"{name} comes before its folder")

    def test_extended_attributes_never_reach_the_archive(self) -> None:
        app = write_app(self.root)
        notices = app / "Contents" / "Resources" / "THIRD-PARTY-NOTICES.md"
        if hasattr(os, "setxattr"):
            with contextlib.suppress(OSError):
                os.setxattr(notices, "user.writ.test", b"1")
        elif shutil.which("xattr"):
            subprocess.run(["xattr", "-w", "com.writ.test", "1", str(notices)], check=False)
        output = self.root / "Writ.app.tar.gz"
        archive_app.create(app, output)
        with tarfile.open(output, "r:gz") as archive:
            headers = [member.pax_headers for member in archive.getmembers()]
            names = archive.getnames()
        self.assertFalse(any("xattr" in key for header in headers for key in header))
        self.assertFalse(any("/._" in name or name.startswith("._") for name in names))

    def test_layout_problems_are_named(self) -> None:
        cases = {
            "two roots": (["Writ.app/", "Writ.app/Contents/Info.plist", "Other.app/x"], "top-level entries"),
            "dot prefix": (["./Writ.app/", "./Writ.app/Contents/Info.plist"], "top-level entries"),
            "apple double": (["Writ.app/", "Writ.app/Contents/Info.plist", "Writ.app/Contents/._Info.plist"], "AppleDouble"),
            "parent reference": (["Writ.app/", "Writ.app/Contents/Info.plist", "Writ.app/../evil"], "not a plain relative path"),
            "no plist": (["Writ.app/", "Writ.app/Contents/"], "Info.plist is missing"),
        }
        for label, (names, expected) in cases.items():
            with self.subTest(case=label):
                path = self.root / f"{label}.tar.gz"
                tar_with(path, names)
                found = archive_app.problems(path, "Writ.app")
                self.assertTrue(any(expected in problem for problem in found), found)

    def test_a_folder_that_is_not_a_bundle_is_refused(self) -> None:
        plain = self.root / "Writ"
        plain.mkdir()
        with self.assertRaises(archive_app.ArchiveError):
            archive_app.create(plain, self.root / "out.tar.gz")
        missing_plist = self.root / "Empty.app"
        missing_plist.mkdir()
        with self.assertRaises(archive_app.ArchiveError):
            archive_app.create(missing_plist, self.root / "out.tar.gz")

    def test_the_command_line_creates_and_checks(self) -> None:
        app = write_app(self.root)
        output = self.root / "Writ.app.tar.gz"
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(archive_app.main(["create", str(app), str(output)]), 0)
            self.assertEqual(archive_app.main(["check", str(output), "Writ.app"]), 0)
        bad = self.root / "bad.tar.gz"
        tar_with(bad, ["Other.app/", "Other.app/Contents/Info.plist"])
        with contextlib.redirect_stderr(io.StringIO()) as stderr:
            self.assertEqual(archive_app.main(["check", str(bad), "Writ.app"]), 1)
        self.assertIn("top-level entries", stderr.getvalue())

    @unittest.skipUnless(ON_MAC, "needs macOS codesign")
    def test_a_signed_bundle_still_verifies_after_the_round_trip(self) -> None:
        app = self.root / "signed" / "Writ.app"
        (app / "Contents" / "MacOS").mkdir(parents=True)
        for name in ("writ-tauri", "writ"):
            executable = app / "Contents" / "MacOS" / name
            shutil.copyfile("/usr/bin/true", executable)
            executable.chmod(0o755)
        with (app / "Contents" / "Info.plist").open("wb") as handle:
            plistlib.dump(
                {"CFBundleExecutable": "writ-tauri", "CFBundleIdentifier": "com.writ.editor.test", "CFBundlePackageType": "APPL"},
                handle,
            )
        subprocess.run(["codesign", "--force", "--sign", "-", "--options", "runtime", str(app / "Contents" / "MacOS" / "writ")], check=True, capture_output=True)
        subprocess.run(["codesign", "--force", "--sign", "-", "--options", "runtime", str(app)], check=True, capture_output=True)
        output = self.root / "Writ.app.tar.gz"
        archive_app.create(app, output)
        installed = self.root / "installed" / "Writ.app"
        installed.parent.mkdir()
        install_like_the_updater(output, installed)
        result = subprocess.run(
            ["codesign", "--verify", "--strict", "--deep", "--verbose=2", str(installed)],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
