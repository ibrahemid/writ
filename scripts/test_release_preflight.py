"""Tests for scripts/release-preflight.sh with every tool it runs replaced by a recorder.

Run from the repository root:
    python3 -m unittest discover -s scripts -p 'test_*.py'
"""

from __future__ import annotations

import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "release-preflight.sh"
VERSION = "0.6.0"

# Stand-ins for the updater key and its Keychain password, named so that CodeQL's
# credential heuristics do not take the stub files they are written to for real ones.
SIGNING_MATERIAL_PLACEHOLDER = "placeholder-signing-material-3b9e61d4"
UNLOCK_PHRASE_PLACEHOLDER = "placeholder-unlock-phrase-c07a52f8"

RECORDER = """#!/usr/bin/env python3
import json, os, pathlib, sys

name, argv = pathlib.Path(sys.argv[0]).name, sys.argv[1:]
if name == "record":
    name, argv = argv[0], argv[1:]
log = pathlib.Path(os.environ["CALL_LOG"])
record = {"name": name, "argv": argv, "env": dict(os.environ)}
(log / f"{len(list(log.iterdir())):04d}-{os.getpid()}.json").write_text(json.dumps(record))
if name == "uname":
    print(os.environ.get("FAKE_UNAME", "Darwin"))
elif name == "cargo" and argv[:1] == ["metadata"]:
    print(json.dumps({"target_directory": os.environ["FAKE_TARGET_DIR"]}))
elif name == "security" and argv[:1] == ["find-generic-password"]:
    keychain = pathlib.Path(os.environ["FAKE_KEYCHAIN"])
    if not keychain.is_file():
        sys.exit(44)
    print(keychain.read_text())
elif name == "gh" and argv[:2] == ["release", "view"]:
    print("v0.5.1")
elif name == "gh" and argv[:1] == ["api"]:
    print("{}")
"""

TOOLS = ("uname", "rustup", "cargo", "lipo", "npx", "node", "pnpm", "minisign", "security", "gh", "docker", "act")
REPO_SCRIPTS = (".github/scripts/archive_app.py", ".github/scripts/release_signing.py", "scripts/gen-third-party-notices.py")
SHELL_SCRIPTS = ("scripts/macos-release.sh", "scripts/build-mac-pkg.sh")


class ReleasePreflightTests(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="writ-preflight-test-")
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        stubs = self.root / "stubs"
        stubs.mkdir()
        (stubs / "record").write_text(RECORDER)
        (stubs / "record").chmod(0o755)
        for name in TOOLS:
            (stubs / name).symlink_to(stubs / "record")
        (stubs / "python3").symlink_to(sys.executable)

        self.repo = self.root / "repo"
        (self.repo / "scripts").mkdir(parents=True)
        (self.repo / ".github" / "scripts").mkdir(parents=True)
        shutil.copy2(SCRIPT, self.repo / "scripts" / SCRIPT.name)
        (self.repo / "Cargo.toml").write_text(f'[workspace.package]\nversion = "{VERSION}"\n')
        for relative in REPO_SCRIPTS:
            (self.repo / relative).write_text(RECORDER)
        for relative in SHELL_SCRIPTS:
            (self.repo / relative).write_text(f'exec "{stubs / "record"}" {pathlib.Path(relative).name} "$@"\n')

        target = self.root / "target"
        for triple in ("aarch64-apple-darwin", "x86_64-apple-darwin"):
            (target / triple / "release").mkdir(parents=True)
            (target / triple / "release" / "writ").write_text("sidecar")

        self.home = self.root / "home"
        self.home.mkdir()
        self.log = self.root / "calls"
        self.log.mkdir()
        self.keychain = self.root / "keychain"
        self.env = {
            "PATH": f"{stubs}:/usr/bin:/bin",
            "HOME": str(self.home),
            "TMPDIR": str(self.root),
            "USER": "writ-release",
            "CALL_LOG": str(self.log),
            "FAKE_TARGET_DIR": str(target),
            "FAKE_KEYCHAIN": str(self.keychain),
        }
        for name in TOOLS:
            self.assertEqual(shutil.which(name, path=self.env["PATH"]), str(stubs / name))

    def install_key_file(self) -> pathlib.Path:
        key_file = self.home / ".tauri" / "writ.key"
        key_file.parent.mkdir()
        key_file.write_text(SIGNING_MATERIAL_PLACEHOLDER)
        return key_file

    def run_preflight(self, **env: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(self.repo / "scripts" / SCRIPT.name)],
            env={**self.env, **env},
            capture_output=True,
            text=True,
            check=False,
        )

    def records(self) -> list[dict]:
        return [json.loads(path.read_text()) for path in sorted(self.log.iterdir())]

    def signing_index(self, records: list[dict]) -> int:
        [index] = [
            i
            for i, record in enumerate(records)
            if record["name"] == "release_signing.py" and record["argv"][:1] == ["sign-payloads"]
        ]
        return index

    def assert_secrets_reach_only_the_signing_command(self, secret_values: list[str], key: str) -> None:
        records = self.records()
        signing = self.signing_index(records)
        for index, record in enumerate(records):
            if index == signing:
                continue
            command = " ".join([record["name"], *record["argv"]])
            with self.subTest(command=command):
                names = sorted(name for name in record["env"] if name.startswith(("TAURI_SIGNING_", "UPDATER_KEY")))
                self.assertEqual(names, [])
                for value in record["env"].values():
                    for secret_value in secret_values:
                        self.assertNotIn(secret_value, value)
        build = next(i for i, r in enumerate(records) if r["name"] == "npx" and r["argv"][:2] == ["tauri", "build"])
        self.assertLess(build, signing)
        signing_env = records[signing]["env"]
        self.assertEqual(signing_env.get("TAURI_SIGNING_PRIVATE_KEY"), key)
        self.assertEqual(signing_env.get("TAURI_SIGNING_PRIVATE_KEY_PASSWORD"), UNLOCK_PHRASE_PLACEHOLDER)

    def test_a_key_file_and_keychain_password_reach_only_the_signing_command(self) -> None:
        key_file = self.install_key_file()
        self.keychain.write_text(UNLOCK_PHRASE_PLACEHOLDER)
        result = self.run_preflight()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_secrets_reach_only_the_signing_command(
            [SIGNING_MATERIAL_PLACEHOLDER, str(key_file), UNLOCK_PHRASE_PLACEHOLDER], str(key_file)
        )

    def test_an_exported_key_and_password_leave_the_environment_before_the_first_gate(self) -> None:
        key_path = str(self.root / "elsewhere.key")
        result = self.run_preflight(
            TAURI_SIGNING_PRIVATE_KEY=SIGNING_MATERIAL_PLACEHOLDER,
            TAURI_SIGNING_PRIVATE_KEY_PASSWORD=UNLOCK_PHRASE_PLACEHOLDER,
            TAURI_SIGNING_PRIVATE_KEY_PATH=key_path,
            UPDATER_KEY="inherited",
            UPDATER_KEY_PASSWORD="inherited",
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_secrets_reach_only_the_signing_command(
            [SIGNING_MATERIAL_PLACEHOLDER, UNLOCK_PHRASE_PLACEHOLDER, key_path], SIGNING_MATERIAL_PLACEHOLDER
        )
        self.assertNotIn("security", [record["name"] for record in self.records()])

    def test_a_missing_key_stops_the_run_before_any_gate(self) -> None:
        self.keychain.write_text(UNLOCK_PHRASE_PLACEHOLDER)
        result = self.run_preflight()
        self.assertEqual(result.returncode, 1)
        self.assertIn(f"no key at {self.home / '.tauri' / 'writ.key'}", result.stderr)
        self.assertEqual([r["name"] for r in self.records() if r["name"] not in {"uname"}], [])

    def test_a_missing_password_stops_the_run_before_any_gate(self) -> None:
        self.install_key_file()
        result = self.run_preflight()
        self.assertEqual(result.returncode, 1)
        self.assertIn("No TAURI_SIGNING_PRIVATE_KEY_PASSWORD env and no Keychain entry", result.stderr)
        self.assertEqual([r["name"] for r in self.records() if r["name"] not in {"uname", "security"}], [])

    def test_off_darwin_the_run_needs_no_key(self) -> None:
        result = self.run_preflight(FAKE_UNAME="Linux")
        self.assertEqual(result.returncode, 0, result.stderr)
        names = [record["name"] for record in self.records()]
        self.assertNotIn("security", names)
        self.assertNotIn("release_signing.py", names)
        self.assertIn("act", names)


if __name__ == "__main__":
    unittest.main()
