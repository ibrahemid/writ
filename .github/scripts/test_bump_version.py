"""Tests for bump_version.py.

Run from the repository root:
    python3 -m unittest discover -s .github/scripts -t .github/scripts
"""

from __future__ import annotations

import json
import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import bump_version  # noqa: E402

RELEASE_JSON = {
    "version": "0.3.4",
    "tag": "v0.3.4",
    "publishedAt": "2026-08-22T16:27:29Z",
    "published": True,
    "notarized": True,
    "hasChecksums": True,
    "platforms": {
        "macos": {
            "dmg": "https://github.com/ibrahemid/writ/releases/download/v0.3.4/Writ_0.3.4_universal.dmg",
            "sha256_dmg": "abc123",
            "size_dmg": 12345,
        },
        "windows": {
            "msi": "https://github.com/ibrahemid/writ/releases/download/v0.3.4/Writ_0.3.4_x64_en-US.msi",
            "sha256_msi": "def456",
            "size_msi": 6789,
        },
    },
}


class BumpReleaseJsonTest(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(tmp.name)
        self.addCleanup(tmp.cleanup)

    def write_release_json(self, data: dict) -> pathlib.Path:
        path = self.root / "release.json"
        path.write_text(json.dumps(data, indent=2) + "\n")
        return path

    def bump(self, new_version: str = "0.5.0", data: dict | None = None) -> dict:
        path = self.write_release_json(RELEASE_JSON if data is None else data)
        bump_version.bump_release_json(path, new_version)
        return json.loads(path.read_text())

    def test_the_version_and_the_tag_move_together(self) -> None:
        bumped = self.bump()

        self.assertEqual(bumped["version"], "0.5.0")
        self.assertEqual(bumped["tag"], "v0.5.0")

    def test_nothing_claims_an_artifact_that_has_not_been_built(self) -> None:
        bumped = self.bump()

        self.assertFalse(bumped["published"])
        self.assertFalse(bumped["notarized"])
        self.assertFalse(bumped["hasChecksums"])
        self.assertEqual(bumped["platforms"]["macos"]["sha256_dmg"], "")
        self.assertEqual(bumped["platforms"]["macos"]["size_dmg"], 0)
        self.assertEqual(bumped["platforms"]["windows"]["sha256_msi"], "")
        self.assertEqual(bumped["platforms"]["windows"]["size_msi"], 0)

    def test_the_download_urls_name_the_new_version(self) -> None:
        bumped = self.bump()

        self.assertEqual(
            bumped["platforms"]["macos"]["dmg"],
            "https://github.com/ibrahemid/writ/releases/download/v0.5.0/Writ_0.5.0_universal.dmg",
        )
        self.assertEqual(
            bumped["platforms"]["windows"]["msi"],
            "https://github.com/ibrahemid/writ/releases/download/v0.5.0/Writ_0.5.0_x64_en-US.msi",
        )

    def test_the_date_is_left_to_the_site_deploy(self) -> None:
        # The bump has no date to give: publishedAt is the release's, and the
        # deploy rewrites the whole file from the published release.
        bumped = self.bump()

        self.assertEqual(bumped["publishedAt"], RELEASE_JSON["publishedAt"])

    def test_a_file_with_no_version_is_refused(self) -> None:
        data = {key: value for key, value in RELEASE_JSON.items() if key != "version"}

        with self.assertRaises(bump_version.BumpError):
            self.bump(data=data)


if __name__ == "__main__":
    unittest.main()
