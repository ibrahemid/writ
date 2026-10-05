import importlib.util
import os
import pathlib
import stat
import tempfile
import unittest
from unittest import mock


SCRIPT_PATH = pathlib.Path(__file__).with_name("packaging_bump_winget.py")
SPEC = importlib.util.spec_from_file_location("packaging_bump_winget", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Could not load {SCRIPT_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


STALE_LOCALE = """ShortDescription: Lightweight text editor.
Description: |-
  Old scratchpad copy.
Moniker: writ
Tags:
  - scratchpad
  - editor
ReleaseNotesUrl: https://github.com/ibrahemid/writ/releases/tag/v0.3.5
ManifestType: defaultLocale
"""


class RewriteLocaleTests(unittest.TestCase):
    def test_replaces_every_public_description_field(self) -> None:
        release_notes_url = "https://github.com/ibrahemid/writ/releases/tag/v1.0.0"

        result = MODULE.rewrite_locale(STALE_LOCALE, release_notes_url)

        self.assertIn(f"ShortDescription: {MODULE.SHORT_DESCRIPTION}", result)
        self.assertIn(f"  {MODULE.DESCRIPTION}\n", result)
        for tag in MODULE.TAGS:
            self.assertIn(f"  - {tag}\n", result)
        self.assertIn(f"ReleaseNotesUrl: {release_notes_url}", result)
        self.assertNotIn("scratchpad", result)
        self.assertNotIn("Lightweight text editor", result)

    def test_refuses_a_locale_without_all_required_fields(self) -> None:
        with self.assertRaises(MODULE.ManifestFormatError):
            MODULE.rewrite_locale("ShortDescription: incomplete\n", "https://example.com")



OLD_PRODUCT_CODE = "{FD0146EA-B2E8-4F19-A5D5-C3F2E4788501}"
NEW_PRODUCT_CODE = "{3C1B8A52-6E0D-4F7A-9B21-0D4E5F6A7B8C}"

PROPERTY_TABLE = (
    "Property\tValue\r\n"
    "s72\tl0\r\n"
    "Property\tProperty\r\n"
    "UpgradeCode\t{AB97CC4D-EEA3-53EE-90D8-3C2767967676}\r\n"
    "Manufacturer\tIbrahem ID\r\n"
    f"ProductCode\t{NEW_PRODUCT_CODE}\r\n"
    "ProductLanguage\t1033\r\n"
    "ProductName\tWrit\r\n"
    "ProductVersion\t0.5.1\r\n"
)

INSTALLER_MANIFEST = f"""PackageIdentifier: ibrahemid.Writ
PackageVersion: 0.5.0
InstallerType: wix
ReleaseDate: 2026-09-26
Installers:
  - Architecture: x64
    InstallerType: wix
    InstallerUrl: https://github.com/ibrahemid/writ/releases/download/v0.5.0/Writ_0.5.0_x64_en-US.msi
    InstallerSha256: 56006b86adda661cc6fccf806fb7d31087c4cc1b48c730621279a6edb8e41079
    InstallerSwitches:
      Silent: /quiet
      SilentWithProgress: /passive
    ProductCode: "{OLD_PRODUCT_CODE}"
ManifestType: installer
ManifestVersion: 1.6.0
"""

LOCALE_MANIFEST = """PackageIdentifier: ibrahemid.Writ
PackageVersion: 0.5.0
PackageLocale: en-US
ShortDescription: Old short description
Description: |-
  Old description.
Moniker: writ
Tags:
  - editor
ReleaseNotesUrl: https://github.com/ibrahemid/writ/releases/tag/v0.5.0
ManifestType: defaultLocale
ManifestVersion: 1.6.0
"""

VERSION_MANIFEST = """PackageIdentifier: ibrahemid.Writ
PackageVersion: 0.5.0
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.6.0
"""

FAKE_MSIINFO = """#!/bin/sh
if [ "$1" != export ] || [ "$3" != Property ] || [ ! -f "$2" ]; then
  echo "unexpected arguments: $*" >&2
  exit 2
fi
if [ -n "$FAKE_MSIINFO_FAIL" ]; then
  echo "$FAKE_MSIINFO_FAIL" >&2
  exit 1
fi
cat "$FAKE_MSIINFO_TABLE"
"""


class ParseProductCodeTests(unittest.TestCase):
    def test_reads_the_product_code_row_of_a_crlf_export(self) -> None:
        self.assertEqual(MODULE.parse_product_code(PROPERTY_TABLE), NEW_PRODUCT_CODE)

    def test_refuses_a_table_without_a_product_code(self) -> None:
        table = PROPERTY_TABLE.replace(f"ProductCode\t{NEW_PRODUCT_CODE}\r\n", "")
        with self.assertRaises(MODULE.MsiReadError):
            MODULE.parse_product_code(table)

    def test_refuses_a_product_code_that_is_not_a_braced_guid(self) -> None:
        table = PROPERTY_TABLE.replace(NEW_PRODUCT_CODE, "3C1B8A52-6E0D-4F7A-9B21-0D4E5F6A7B8C")
        with self.assertRaises(MODULE.MsiReadError):
            MODULE.parse_product_code(table)

    def test_refuses_a_table_with_two_product_codes(self) -> None:
        table = PROPERTY_TABLE + f"ProductCode\t{OLD_PRODUCT_CODE}\r\n"
        with self.assertRaises(MODULE.MsiReadError):
            MODULE.parse_product_code(table)


class RewriteInstallerTests(unittest.TestCase):
    def test_refuses_a_manifest_without_a_product_code_line(self) -> None:
        manifest = INSTALLER_MANIFEST.replace(f'    ProductCode: "{OLD_PRODUCT_CODE}"\n', "")
        with self.assertRaises(MODULE.ManifestFormatError):
            MODULE.rewrite_installer(
                manifest,
                installer_url="https://example.com/Writ.msi",
                sha_msi="0" * 64,
                release_date="2026-10-05",
                product_code=NEW_PRODUCT_CODE,
            )


class BumpFromReleasedMsiTests(unittest.TestCase):
    def setUp(self) -> None:
        previous_cwd = os.getcwd()
        tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(tmp.name)
        os.chdir(self.root)
        self.addCleanup(tmp.cleanup)
        self.addCleanup(os.chdir, previous_cwd)

        source = self.root / MODULE.MANIFEST_ROOT / "0.5.0"
        source.mkdir(parents=True)
        (source / "ibrahemid.Writ.installer.yaml").write_text(INSTALLER_MANIFEST)
        (source / "ibrahemid.Writ.locale.en-US.yaml").write_text(LOCALE_MANIFEST)
        (source / "ibrahemid.Writ.yaml").write_text(VERSION_MANIFEST)

        self.msi = self.root / "artifacts" / "Writ_0.5.1_x64_en-US.msi"
        self.msi.parent.mkdir()
        self.msi.write_bytes(b"msi")

        self.table = self.root / "property.idt"
        self.table.write_text(PROPERTY_TABLE, newline="")

        self.bin = self.root / "bin"
        self.bin.mkdir()
        msiinfo = self.bin / "msiinfo"
        msiinfo.write_text(FAKE_MSIINFO)
        msiinfo.chmod(msiinfo.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

    def environment(self, **overrides: str) -> dict[str, str]:
        env = {
            "VERSION": "0.5.1",
            "SHA_MSI": "a" * 64,
            "RELEASE_DATE": "2026-10-05",
            "MSI_PATH": str(self.msi),
            "PATH": f"{self.bin}{os.pathsep}{os.environ.get('PATH', '')}",
            "FAKE_MSIINFO_TABLE": str(self.table),
        }
        env.update(overrides)
        return env

    def run_bump(self, **overrides: str) -> None:
        with mock.patch.dict(os.environ, self.environment(**overrides)):
            MODULE.main()

    def target(self) -> pathlib.Path:
        return self.root / MODULE.MANIFEST_ROOT / "0.5.1"

    def test_installer_manifest_takes_the_product_code_of_the_released_msi(self) -> None:
        self.run_bump()

        installer = (self.target() / "ibrahemid.Writ.installer.yaml").read_text()
        self.assertIn(f'    ProductCode: "{NEW_PRODUCT_CODE}"\n', installer)
        self.assertNotIn(OLD_PRODUCT_CODE, installer)
        self.assertIn("PackageVersion: 0.5.1\n", installer)
        self.assertIn(
            "InstallerUrl: https://github.com/ibrahemid/writ/releases/download/v0.5.1/Writ_0.5.1_x64_en-US.msi\n",
            installer,
        )
        self.assertIn(f"InstallerSha256: {'a' * 64}\n", installer)
        self.assertIn("ReleaseDate: 2026-10-05\n", installer)

    def test_refuses_to_inherit_the_previous_product_code(self) -> None:
        self.table.write_text(
            PROPERTY_TABLE.replace(f"ProductCode\t{NEW_PRODUCT_CODE}\r\n", ""), newline=""
        )

        with self.assertRaises(MODULE.MsiReadError):
            self.run_bump()
        self.assertFalse(self.target().exists())

    def test_reports_what_msiinfo_printed_when_it_fails(self) -> None:
        with self.assertRaisesRegex(MODULE.MsiReadError, "not an MSI database"):
            self.run_bump(FAKE_MSIINFO_FAIL="not an MSI database")
        self.assertFalse(self.target().exists())

    def test_names_msitools_when_msiinfo_is_not_installed(self) -> None:
        empty_bin = self.root / "empty-bin"
        empty_bin.mkdir()

        with self.assertRaisesRegex(MODULE.MsiReadError, "msitools"):
            self.run_bump(PATH=str(empty_bin))
        self.assertFalse(self.target().exists())

    def test_refuses_a_missing_msi(self) -> None:
        with self.assertRaises(MODULE.MsiReadError):
            self.run_bump(MSI_PATH=str(self.root / "artifacts" / "missing.msi"))
        self.assertFalse(self.target().exists())


if __name__ == "__main__":
    unittest.main()
