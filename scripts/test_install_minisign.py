"""Tests for scripts/install-minisign.sh with stubbed host and download.

Run from the repository root:
    python3 -m unittest discover -s scripts -p 'test_*.py'
"""

from __future__ import annotations

import hashlib
import io
import pathlib
import re
import shutil
import subprocess
import tarfile
import tempfile
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "install-minisign.sh"
WORKFLOWS = REPO_ROOT / ".github" / "workflows"
PIN = re.compile(r'^MINISIGN_SHA256="([0-9a-f]{64})"$', re.MULTILINE)
VERSION = re.compile(r'^MINISIGN_VERSION="([^"]+)"$', re.MULTILINE)

UNAME = """#!/bin/sh
case "$1" in
  -m) echo "$FAKE_MACHINE" ;;
  *) echo "$FAKE_SYSTEM" ;;
esac
"""

CURL = """#!/bin/sh
printf '%s\\n' "$*" >> "$CURL_LOG"
while [ "$#" -gt 0 ]; do
  if [ "$1" = "-o" ]; then cp "$FAKE_ARCHIVE" "$2"; fi
  shift
done
"""

FAKE_MINISIGN = b"#!/bin/sh\necho 'minisign 0.12'\n"


def fake_archive(path: pathlib.Path) -> str:
    with tarfile.open(path, "w:gz") as archive:
        info = tarfile.TarInfo("minisign-linux/x86_64/minisign")
        info.size = len(FAKE_MINISIGN)
        info.mode = 0o755
        archive.addfile(info, io.BytesIO(FAKE_MINISIGN))
        extra = tarfile.TarInfo("minisign-linux/aarch64/minisign")
        extra.size = len(FAKE_MINISIGN)
        extra.mode = 0o755
        archive.addfile(extra, io.BytesIO(FAKE_MINISIGN))
    return hashlib.sha256(path.read_bytes()).hexdigest()


class InstallMinisignTests(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="writ-install-minisign-test-")
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        stubs = self.root / "stubs"
        stubs.mkdir()
        for name, body in {"uname": UNAME, "curl": CURL}.items():
            stub = stubs / name
            stub.write_text(body)
            stub.chmod(0o755)
        self.archive = self.root / "download.tar.gz"
        self.archive_sha256 = fake_archive(self.archive)
        self.curl_log = self.root / "curl.log"
        self.env = {
            "PATH": f"{stubs}:/usr/bin:/bin:/usr/sbin:/sbin",
            "HOME": str(self.root),
            "FAKE_SYSTEM": "Linux",
            "FAKE_MACHINE": "x86_64",
            "FAKE_ARCHIVE": str(self.archive),
            "CURL_LOG": str(self.curl_log),
        }
        self.assertIsNotNone(shutil.which("sha256sum", path=self.env["PATH"]), "sha256sum is not on the test PATH")
        self.dest = self.root / "minisign"

    def run_script(self, script: pathlib.Path = SCRIPT, *args: str, **env: str) -> subprocess.CompletedProcess[str]:
        arguments = list(args) if args else [str(self.dest)]
        return subprocess.run(
            ["bash", str(script), *arguments], env={**self.env, **env}, capture_output=True, text=True, check=False
        )

    def pinned_to_the_fake_archive(self) -> pathlib.Path:
        text = SCRIPT.read_text()
        self.assertEqual(len(PIN.findall(text)), 1)
        copy = self.root / "install-minisign.sh"
        copy.write_text(PIN.sub(f'MINISIGN_SHA256="{self.archive_sha256}"', text))
        return copy

    def downloads(self) -> list[str]:
        return self.curl_log.read_text().splitlines() if self.curl_log.exists() else []

    def test_the_pinned_archive_installs_and_the_binary_path_is_printed(self) -> None:
        result = self.run_script(self.pinned_to_the_fake_archive())
        self.assertEqual(result.returncode, 0, result.stderr)
        binary = self.dest / "minisign-linux" / "x86_64" / "minisign"
        self.assertEqual(result.stdout, f"{binary}\n")
        self.assertTrue(binary.is_file())
        self.assertIn("minisign 0.12", result.stderr)
        self.assertFalse((self.dest / "minisign-linux" / "aarch64").exists())
        [download] = self.downloads()
        version = VERSION.search(SCRIPT.read_text()).group(1)
        self.assertIn(
            f"https://github.com/jedisct1/minisign/releases/download/{version}/minisign-{version}-linux.tar.gz",
            download.split(),
        )

    def test_an_archive_with_another_checksum_is_removed_and_nothing_is_installed(self) -> None:
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        pinned = PIN.search(SCRIPT.read_text()).group(1)
        self.assertIn(f"has SHA-256 {self.archive_sha256}, expected {pinned}", result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertEqual(list(self.dest.iterdir()), [])

    def test_another_host_is_refused_before_anything_downloads(self) -> None:
        for system, machine in (("Darwin", "arm64"), ("Linux", "aarch64")):
            with self.subTest(system=system, machine=machine):
                result = self.run_script(FAKE_SYSTEM=system, FAKE_MACHINE=machine)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(f"not {system} {machine}", result.stderr)
                self.assertEqual(self.downloads(), [])
                self.assertFalse(self.dest.exists())

    def test_the_destination_is_required(self) -> None:
        for args in ((), (str(self.dest), "extra")):
            with self.subTest(args=args):
                result = subprocess.run(
                    ["bash", str(SCRIPT), *args], env=self.env, capture_output=True, text=True, check=False
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("usage: install-minisign.sh <dir>", result.stderr)
                self.assertEqual(self.downloads(), [])

    def test_the_pin_is_the_published_linux_build_of_the_named_version(self) -> None:
        text = SCRIPT.read_text()
        self.assertEqual(VERSION.search(text).group(1), "0.12")
        self.assertEqual(PIN.search(text).group(1), "9a599b48ba6eb7b1e80f12f36b94ceca7c00b7a5173c95c3efc88d9822957e73")

    def test_every_workflow_installs_minisign_only_through_this_script(self) -> None:
        callers = []
        for workflow in sorted(WORKFLOWS.glob("*.yml")):
            text = workflow.read_text()
            self.assertNotIn("jedisct1/minisign/releases", text, workflow.name)
            if "bash scripts/install-minisign.sh" in text:
                callers.append(workflow.name)
        self.assertEqual(callers, ["ci.yml", "release.yml"])


if __name__ == "__main__":
    unittest.main()
