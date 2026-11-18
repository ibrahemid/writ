"""Tests for the bundle settings that carry installs made under the old product name.

Run from the repository root:
    python3 -m unittest discover -s scripts -p 'test_*.py'
"""

from __future__ import annotations

import json
import pathlib
import unittest

TAURI_CONF = pathlib.Path(__file__).resolve().parent.parent / "src-tauri" / "tauri.conf.json"
UPGRADE_CODE = "ab97cc4d-eea3-53ee-90d8-3c2767967676"


def load_tauri_conf() -> dict:
    return json.loads(TAURI_CONF.read_text())


class LegacyBundleSettingsTests(unittest.TestCase):
    def test_the_msi_upgrade_code_is_pinned(self) -> None:
        self.assertEqual(load_tauri_conf()["bundle"]["windows"]["wix"]["upgradeCode"], UPGRADE_CODE)

    def test_the_deb_replaces_the_old_package(self) -> None:
        deb = load_tauri_conf()["bundle"]["linux"]["deb"]
        for field in ("conflicts", "replaces", "provides"):
            with self.subTest(field=field):
                self.assertEqual(deb[field], ["writ"])

    def test_both_link_schemes_are_registered(self) -> None:
        schemes = load_tauri_conf()["plugins"]["deep-link"]["desktop"]["schemes"]
        self.assertEqual(schemes, ["jotra", "writ"])


if __name__ == "__main__":
    unittest.main()
