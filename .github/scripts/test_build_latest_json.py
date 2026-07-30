"""Tests for build_latest_json.py.

Run from the repository root:
    python3 -m unittest discover -s .github/scripts -t .github/scripts
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import pathlib
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import build_latest_json  # noqa: E402

RELEASE_ENV = {"VERSION": "0.3.0", "TAG": "v0.3.0", "REPO": "ibrahemid/writ"}

MAC_BUNDLE = "Writ_universal.app.tar.gz"
WINDOWS_MSI = "Writ_0.3.0_x64_en-US.msi"
LINUX_APPIMAGE = "Writ_0.3.0_amd64.AppImage"
LINUX_DEB = "Writ_0.3.0_amd64.deb"

FULL_ASSET_SET = (MAC_BUNDLE, WINDOWS_MSI, LINUX_APPIMAGE, LINUX_DEB)

ALL_PLATFORM_KEYS = {
    "darwin-aarch64",
    "darwin-x86_64",
    "windows-x86_64",
    "linux-x86_64",
    "linux-x86_64-deb",
}


class BuildLatestJsonTest(unittest.TestCase):
    def setUp(self) -> None:
        previous_cwd = os.getcwd()
        tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(tmp.name)
        os.chdir(self.root)
        self.addCleanup(tmp.cleanup)
        self.addCleanup(os.chdir, previous_cwd)

    def write_asset(self, name: str, *, signed: bool = True) -> None:
        (self.root / name).write_bytes(b"payload")
        if signed:
            (self.root / f"{name}.sig").write_text(f"signature-for-{name}\n")

    def write_assets(self, *names: str) -> None:
        for name in names:
            self.write_asset(name)

    def run_script(self, env: dict[str, str] = RELEASE_ENV) -> tuple[int, dict, str]:
        stdout, stderr = io.StringIO(), io.StringIO()
        with mock.patch.dict(os.environ, env, clear=True):
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                code = build_latest_json.main()
        manifest_path = self.root / "latest.json"
        manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {}
        return code, manifest, stderr.getvalue()

    def test_full_asset_set_emits_every_platform_key(self) -> None:
        self.write_assets(*FULL_ASSET_SET)

        code, manifest, _ = self.run_script()

        self.assertEqual(code, 0)
        self.assertEqual(set(manifest["platforms"]), ALL_PLATFORM_KEYS)
        self.assertEqual(manifest["version"], "0.3.0")

    def test_deb_entry_carries_the_deb_url_and_its_signature(self) -> None:
        self.write_assets(*FULL_ASSET_SET)

        _, manifest, _ = self.run_script()

        deb = manifest["platforms"]["linux-x86_64-deb"]
        self.assertEqual(
            deb["url"],
            f"https://github.com/ibrahemid/writ/releases/download/v0.3.0/{LINUX_DEB}",
        )
        self.assertEqual(deb["signature"], f"signature-for-{LINUX_DEB}")
        self.assertTrue(
            manifest["platforms"]["linux-x86_64"]["url"].endswith(LINUX_APPIMAGE)
        )

    def test_mac_bundle_serves_both_darwin_architectures(self) -> None:
        self.write_assets(*FULL_ASSET_SET)

        _, manifest, _ = self.run_script()

        self.assertEqual(
            manifest["platforms"]["darwin-aarch64"],
            manifest["platforms"]["darwin-x86_64"],
        )

    def test_missing_deb_fails_the_manifest(self) -> None:
        self.write_assets(MAC_BUNDLE, WINDOWS_MSI, LINUX_APPIMAGE)

        code, manifest, stderr = self.run_script()

        self.assertEqual(code, 1)
        self.assertNotIn("linux-x86_64-deb", manifest["platforms"])
        self.assertIn("linux-x86_64-deb", stderr)

    def test_unsigned_deb_fails_the_manifest(self) -> None:
        self.write_assets(MAC_BUNDLE, WINDOWS_MSI, LINUX_APPIMAGE)
        self.write_asset(LINUX_DEB, signed=False)

        code, manifest, stderr = self.run_script()

        self.assertEqual(code, 1)
        self.assertNotIn("linux-x86_64-deb", manifest["platforms"])
        self.assertIn("linux-x86_64-deb", stderr)

    def test_unsigned_appimage_fails_the_manifest(self) -> None:
        self.write_assets(MAC_BUNDLE, WINDOWS_MSI, LINUX_DEB)
        self.write_asset(LINUX_APPIMAGE, signed=False)

        code, manifest, stderr = self.run_script()

        self.assertEqual(code, 1)
        self.assertNotIn("linux-x86_64", manifest["platforms"])
        self.assertIn("linux-x86_64", stderr)

    def test_amd64_deb_wins_over_a_deb_that_sorts_earlier(self) -> None:
        self.write_assets(*FULL_ASSET_SET)
        self.write_asset("Writ_0.3.0_aarch64.deb")

        code, manifest, _ = self.run_script()

        self.assertEqual(code, 0)
        self.assertTrue(
            manifest["platforms"]["linux-x86_64-deb"]["url"].endswith(LINUX_DEB)
        )

    def test_a_foreign_arch_deb_is_published_under_the_x86_64_key(self) -> None:
        # Current behaviour, asserted so a change to it is deliberate. The
        # `\.deb$` fallback has no architecture guard, so with no amd64 deb
        # present it serves whatever deb it finds under an x86_64 key. Safe
        # only while release.yml builds a single amd64 deb
        # (.github/workflows/release.yml matrix: ubuntu-22.04,
        # x86_64-unknown-linux-gnu). Adding an arm64 deb to the matrix makes
        # this mis-serve, and this test is where that shows up.
        self.write_assets(MAC_BUNDLE, WINDOWS_MSI, LINUX_APPIMAGE)
        self.write_asset("Writ_0.3.0_aarch64.deb")

        code, manifest, _ = self.run_script()

        self.assertEqual(code, 0)
        self.assertTrue(
            manifest["platforms"]["linux-x86_64-deb"]["url"].endswith(
                "Writ_0.3.0_aarch64.deb"
            )
        )

    def test_missing_env_var_fails_before_writing_a_manifest(self) -> None:
        self.write_assets(*FULL_ASSET_SET)

        code, manifest, stderr = self.run_script(env={"TAG": "v0.3.0", "REPO": "x/y"})

        self.assertEqual(code, 1)
        self.assertEqual(manifest, {})
        self.assertIn("VERSION", stderr)


if __name__ == "__main__":
    unittest.main()
