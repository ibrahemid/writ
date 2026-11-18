"""Tests for what the AUR package keeps for installs of the old package name.

Run from the repository root:
    python3 -m unittest discover -s scripts -p 'test_*.py'
"""

from __future__ import annotations

import importlib.util
import os
import pathlib
import platform
import re
import unittest

FIXTURE_PATH = pathlib.Path(__file__).with_name("test_packaging_bump_aur.py")
SPEC = importlib.util.spec_from_file_location("test_packaging_bump_aur_fixture", FIXTURE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Could not load {FIXTURE_PATH}")
FIXTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURE)

MODULE = FIXTURE.MODULE
SRCINFO = FIXTURE.ROOT / "packaging/aur/writ-bin/.SRCINFO"


class LegacySrcinfoTests(unittest.TestCase):
    def test_the_package_provides_and_conflicts_with_both_names(self) -> None:
        text = MODULE.srcinfo_text(FIXTURE.REPO_PKGBUILD.read_text(), "9.9.9", FIXTURE.SHA, FIXTURE.LICENSE_SHA)

        self.assertEqual(re.findall(r"\tprovides = (\S+)", text), ["jotra", "writ"])
        self.assertEqual(re.findall(r"\tconflicts = (\S+)", text), ["jotra", "writ"])

    def test_the_committed_srcinfo_lists_both_names(self) -> None:
        text = SRCINFO.read_text()

        self.assertEqual(re.findall(r"\tprovides = (\S+)", text), ["jotra", "writ"])
        self.assertEqual(re.findall(r"\tconflicts = (\S+)", text), ["jotra", "writ"])


@unittest.skipIf(FIXTURE.BASH is None, "bash is not installed")
class LegacyCommandWrapperTests(FIXTURE.PkgbuildFixture):
    def test_the_legacy_command_keeps_a_gui_path_the_person_set(self) -> None:
        self.assert_gui_variable_precedence("writ")


@unittest.skipIf(FIXTURE.BASH is None, "bash is not installed")
@unittest.skipUnless(platform.system() == "Linux", "package() uses GNU install and sed")
class LegacyPackageTests(FIXTURE.PkgbuildFixture):
    def setUp(self) -> None:
        super().setUp()
        self.write_payload()
        result = self.run_function("package")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_the_legacy_command_runs_its_own_copy_of_the_cli(self) -> None:
        copy = self.pkgdir / "opt/jotra/usr/bin/writ"
        self.assertEqual(copy.read_text(), "cli\n")
        self.assertFalse(copy.is_symlink())
        wrapper = self.pkgdir / "usr/bin/writ"
        self.assertTrue(os.access(wrapper, os.X_OK))
        text = wrapper.read_text()
        self.assertIn(': "${JOTRA_GUI_BIN:=${WRIT_GUI_BIN:-/opt/jotra/AppRun}}"\n', text)
        self.assertIn("export JOTRA_GUI_BIN\n", text)
        self.assertIn("JOTRA_COMMAND_NAME=writ\nexport JOTRA_COMMAND_NAME\n", text)
        self.assertIn('exec "/opt/jotra/usr/bin/writ" "$@"\n', text)

    def test_the_legacy_launcher_links_to_the_launcher(self) -> None:
        link = self.pkgdir / "usr/bin/writ-tauri"
        self.assertTrue(link.is_symlink())
        self.assertEqual(os.readlink(link), "jotra-tauri")

    def test_the_legacy_desktop_entry_is_hidden_and_opens_the_same_files(self) -> None:
        applications = self.pkgdir / "usr/share/applications"
        entry = (applications / "jotra.desktop").read_text().splitlines()
        legacy = (applications / "writ.desktop").read_text().splitlines()

        self.assertEqual(legacy[0], "[Desktop Entry]")
        self.assertEqual(legacy[1], "NoDisplay=true")
        self.assertEqual(legacy.count("NoDisplay=true"), 1)
        self.assertEqual([line for line in legacy if line != "NoDisplay=true"], entry)
        self.assertEqual((applications / "writ.desktop").stat().st_mode & 0o777, 0o644)


if __name__ == "__main__":
    unittest.main()
