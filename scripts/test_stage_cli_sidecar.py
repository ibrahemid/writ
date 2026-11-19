"""Tests for scripts/stage-cli-sidecar.sh.

Run from the repository root:
    python3 -m unittest discover -s scripts -p 'test_*.py'
"""

from __future__ import annotations

import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "stage-cli-sidecar.sh"
CONF = REPO_ROOT / "src-tauri" / "tauri.conf.json"


class StageCliSidecarTests(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="jotra-stage-sidecar-test-")
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.repo = self.root / "repo"
        (self.repo / "scripts").mkdir(parents=True)
        (self.repo / "src-tauri").mkdir()
        shutil.copy2(SCRIPT, self.repo / "scripts" / SCRIPT.name)
        self.built = self.root / "build" / "command"
        self.built.parent.mkdir()
        self.built.write_bytes(b"\x7fELF command build")

    def write_conf(self, external_bin: list[str]) -> None:
        conf = json.loads(CONF.read_text())
        conf["bundle"]["externalBin"] = external_bin
        (self.repo / "src-tauri" / "tauri.conf.json").write_text(json.dumps(conf, indent=4))

    def stage(self, built: pathlib.Path, triple: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(self.repo / "scripts" / SCRIPT.name), str(built), triple],
            capture_output=True,
            text=True,
            check=False,
        )

    def test_every_bundled_name_in_the_repository_config_is_staged(self) -> None:
        shutil.copy2(CONF, self.repo / "src-tauri" / "tauri.conf.json")
        names = json.loads(CONF.read_text())["bundle"]["externalBin"]
        self.assertGreaterEqual(len(names), 2)

        result = self.stage(self.built, "aarch64-apple-darwin")
        self.assertEqual(result.returncode, 0, result.stderr)
        for name in names:
            staged = self.repo / "src-tauri" / f"{name}-aarch64-apple-darwin"
            with self.subTest(name=name):
                self.assertEqual(staged.read_bytes(), self.built.read_bytes())
                self.assertTrue(os.access(staged, os.X_OK))

    def test_a_windows_triple_stages_with_the_windows_suffix(self) -> None:
        self.write_conf(["binaries/first", "binaries/second"])
        result = self.stage(self.built, "x86_64-pc-windows-msvc")
        self.assertEqual(result.returncode, 0, result.stderr)
        staged = sorted(path.name for path in (self.repo / "src-tauri" / "binaries").iterdir())
        self.assertEqual(
            staged,
            ["first-x86_64-pc-windows-msvc.exe", "second-x86_64-pc-windows-msvc.exe"],
        )

    def test_a_single_line_list_is_read(self) -> None:
        conf = json.loads(CONF.read_text())
        conf["bundle"]["externalBin"] = ["binaries/first", "binaries/second"]
        (self.repo / "src-tauri" / "tauri.conf.json").write_text(json.dumps(conf))
        result = self.stage(self.built, "x86_64-unknown-linux-gnu")
        self.assertEqual(result.returncode, 0, result.stderr)
        for name in ("first", "second"):
            with self.subTest(name=name):
                self.assertTrue((self.repo / "src-tauri" / "binaries" / f"{name}-x86_64-unknown-linux-gnu").is_file())

    def test_a_missing_build_stops_before_anything_is_staged(self) -> None:
        self.write_conf(["binaries/first"])
        result = self.stage(self.root / "absent", "aarch64-apple-darwin")
        self.assertEqual(result.returncode, 1)
        self.assertIn("no build at", result.stderr)
        self.assertFalse((self.repo / "src-tauri" / "binaries").exists())

    def test_a_config_without_bundled_names_fails(self) -> None:
        self.write_conf([])
        result = self.stage(self.built, "aarch64-apple-darwin")
        self.assertEqual(result.returncode, 1)
        self.assertIn("lists no bundle.externalBin entries", result.stderr)

    def test_a_wrong_argument_count_prints_the_usage(self) -> None:
        result = subprocess.run(
            ["bash", str(self.repo / "scripts" / SCRIPT.name), str(self.built)],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("usage:", result.stderr)


if __name__ == "__main__":
    unittest.main()
