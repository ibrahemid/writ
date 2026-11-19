"""Tests for how the winget bump keeps the package published under the old product name.

Run from the repository root:
    python3 -m unittest discover -s scripts -p 'test_*.py'
"""

from __future__ import annotations

import importlib.util
import os
import pathlib
import shutil
import stat
import tempfile
import unittest
from unittest import mock

FIXTURE_PATH = pathlib.Path(__file__).with_name("test_packaging_bump_winget.py")
SPEC = importlib.util.spec_from_file_location("test_packaging_bump_winget_fixture", FIXTURE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Could not load {FIXTURE_PATH}")
FIXTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURE)
MODULE = FIXTURE.MODULE

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parent.parent
LEGACY_RELEASE = "1.0.1"
NEXT_RELEASE = "1.0.2"


class LegacyTagTests(unittest.TestCase):
    def test_the_old_name_stays_a_search_tag(self) -> None:
        self.assertEqual(MODULE.LEGACY_TAG, "writ")
        result = MODULE.rewrite_locale(FIXTURE.STALE_LOCALE, "https://example.com")
        self.assertIn("  - writ\nReleaseNotesUrl:", result)
        self.assertEqual(result.count("  - writ\n"), 1)


class BumpFromTheLastLegacyReleaseTests(unittest.TestCase):
    def setUp(self) -> None:
        previous_cwd = os.getcwd()
        tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(tmp.name)
        os.chdir(self.root)
        self.addCleanup(tmp.cleanup)
        self.addCleanup(os.chdir, previous_cwd)

        shutil.copytree(
            REPOSITORY_ROOT / MODULE.MANIFEST_ROOT / LEGACY_RELEASE,
            self.root / MODULE.MANIFEST_ROOT / LEGACY_RELEASE,
        )

        msi = self.root / "artifacts" / f"Jotra_{NEXT_RELEASE}_x64_en-US.msi"
        msi.parent.mkdir()
        msi.write_bytes(b"msi")
        table = self.root / "property.idt"
        table.write_text(FIXTURE.PROPERTY_TABLE, newline="")
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        msiinfo = bin_dir / "msiinfo"
        msiinfo.write_text(FIXTURE.FAKE_MSIINFO)
        msiinfo.chmod(msiinfo.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

        environment = {
            "VERSION": NEXT_RELEASE,
            "SHA_MSI": "b" * 64,
            "RELEASE_DATE": "2026-11-18",
            "MSI_PATH": str(msi),
            "PATH": f"{bin_dir}{os.pathsep}{os.environ.get('PATH', '')}",
            "FAKE_MSIINFO_TABLE": str(table),
        }
        with mock.patch.dict(os.environ, environment):
            MODULE.main()
        self.target = self.root / MODULE.MANIFEST_ROOT / NEXT_RELEASE

    def read_manifest(self, suffix: str) -> str:
        return (self.target / f"ibrahemid.Writ{suffix}").read_text()

    def test_every_manifest_keeps_the_package_identifier(self) -> None:
        for suffix in (".yaml", ".installer.yaml", ".locale.en-US.yaml"):
            with self.subTest(manifest=suffix):
                manifest = self.read_manifest(suffix)
                self.assertIn("PackageIdentifier: ibrahemid.Writ\n", manifest)
                self.assertIn(f"PackageVersion: {NEXT_RELEASE}\n", manifest)

    def test_the_locale_takes_the_new_name_and_keeps_the_old_one_as_a_tag(self) -> None:
        locale = self.read_manifest(".locale.en-US.yaml")

        self.assertIn("PackageName: Jotra\n", locale)
        self.assertIn("Moniker: jotra\n", locale)
        self.assertIn("  - writ\n", locale)
        self.assertNotIn("PackageName: Writ", locale)
        self.assertNotIn("Moniker: writ", locale)

    def test_the_installer_names_the_entry_windows_lists(self) -> None:
        installer = self.read_manifest(".installer.yaml")

        self.assertIn(FIXTURE.APPS_AND_FEATURES_ENTRIES, installer)
        self.assertEqual(installer.count("AppsAndFeaturesEntries:"), 1)
        self.assertIn(
            f"releases/download/v{NEXT_RELEASE}/Jotra_{NEXT_RELEASE}_x64_en-US.msi\n",
            installer,
        )


if __name__ == "__main__":
    unittest.main()
