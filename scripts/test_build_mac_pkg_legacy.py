"""Tests for the install scripts scripts/build-mac-pkg.sh writes, run against a
scratch volume with the macOS tools they call stubbed.

Run from the repository root:
    python3 -m unittest discover -s scripts -p 'test_*.py'
"""

from __future__ import annotations

import importlib.util
import pathlib
import plistlib
import subprocess
import sys
import unittest

FIXTURE_PATH = pathlib.Path(__file__).with_name("test_build_mac_pkg.py")
SPEC = importlib.util.spec_from_file_location("test_build_mac_pkg_fixture", FIXTURE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Could not load {FIXTURE_PATH}")
FIXTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURE)

BUNDLE_ID = "com.writ.editor"
PATTERN = r"Jotra\.app/Contents/MacOS/"
LEGACY_PATTERN = r"Writ\.app/Contents/MacOS/"

PLUTIL = f"""#!{sys.executable}
import plistlib
import sys

if sys.argv[1:6] != ["-extract", "CFBundleIdentifier", "raw", "-o", "-"] or len(sys.argv) != 7:
    sys.exit(2)
try:
    with open(sys.argv[6], "rb") as handle:
        print(plistlib.load(handle)["CFBundleIdentifier"])
except (OSError, KeyError, plistlib.InvalidFileException):
    sys.exit(1)
"""

HOOK_STUBS = {
    "osascript": FIXTURE.RECORDER + "exit 0\n",
    "pgrep": FIXTURE.RECORDER + '[ -n "${FAKE_RUNNING:-}" ]\n',
    "pkill": FIXTURE.RECORDER + "exit 0\n",
    "chflags": FIXTURE.RECORDER + "exit 0\n",
    "sudo": FIXTURE.RECORDER + "exit 0\n",
    "stat": FIXTURE.RECORDER + "printf '%s\\n' \"$FAKE_CONSOLE_USER\"\n",
    "sleep": "#!/bin/sh\nexit 0\n",
    "plutil": PLUTIL,
}


class LegacyInstallScriptTests(FIXTURE.MacPkgFixture):
    def setUp(self) -> None:
        super().setUp()
        self.scripts = self.build_package() / "pkgbuild-scripts"
        hook_stubs = self.root / "hook-stubs"
        hook_stubs.mkdir()
        for name, body in HOOK_STUBS.items():
            stub = hook_stubs / name
            stub.write_text(body)
            stub.chmod(0o755)
        self.hook_log = self.root / "hook-calls"
        self.hook_log.mkdir()
        self.hook_env = {
            "PATH": f"{hook_stubs}:/usr/bin:/bin",
            "HOME": str(self.root),
            "CALL_LOG": str(self.hook_log),
            "FAKE_CONSOLE_USER": "alice",
        }
        self.volume = self.root / "volume"
        self.applications = self.volume / "Applications"
        self.applications.mkdir(parents=True)
        self.app = self.applications / "Jotra.app"
        self.legacy_app = self.applications / "Writ.app"

    def run_hook(self, name: str, **env: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(self.scripts / name), str(self.root / "Jotra.pkg"), "/", str(self.volume), "/"],
            env={**self.hook_env, **env},
            capture_output=True,
            text=True,
            check=False,
        )

    def hook_calls(self, tool: str) -> list[list[str]]:
        records = [record.read_bytes().decode().split("\0")[:-1] for record in sorted(self.hook_log.iterdir())]
        return [record[1:] for record in records if record[0] == tool]

    def make_bundle(self, path: pathlib.Path, identifier: str) -> pathlib.Path:
        contents = path / "Contents"
        (contents / "MacOS").mkdir(parents=True)
        with (contents / "Info.plist").open("wb") as handle:
            plistlib.dump({"CFBundleIdentifier": identifier}, handle)
        (contents / "MacOS" / "marker").write_text(identifier)
        return path

    def test_preinstall_quits_by_identifier_then_kills_both_bundle_names(self) -> None:
        result = self.run_hook("preinstall", FAKE_RUNNING="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        [quit_call] = self.hook_calls("osascript")
        self.assertIn(f'tell application id "{BUNDLE_ID}" to quit', quit_call[-1])
        self.assertIn(["-f", PATTERN], self.hook_calls("pgrep"))
        self.assertEqual(self.hook_calls("pkill"), [["-9", "-f", PATTERN], ["-9", "-f", LEGACY_PATTERN]])

    def test_preinstall_checks_both_bundle_names_before_it_kills_nothing(self) -> None:
        result = self.run_hook("preinstall")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.hook_calls("pgrep"), [["-f", PATTERN], ["-f", LEGACY_PATTERN]])
        self.assertEqual(self.hook_calls("pkill"), [])

    def test_a_legacy_bundle_becomes_the_new_bundle_when_that_is_absent(self) -> None:
        self.make_bundle(self.legacy_app, BUNDLE_ID)
        result = self.run_hook("preinstall")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.legacy_app.exists() or self.legacy_app.is_symlink())
        self.assertEqual((self.app / "Contents" / "MacOS" / "marker").read_text(), BUNDLE_ID)

    def test_a_legacy_bundle_is_removed_when_the_new_bundle_is_present(self) -> None:
        self.make_bundle(self.legacy_app, BUNDLE_ID)
        self.make_bundle(self.app, BUNDLE_ID)
        (self.app / "Contents" / "MacOS" / "marker").write_text("current")
        result = self.run_hook("preinstall")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.legacy_app.exists() or self.legacy_app.is_symlink())
        self.assertEqual((self.app / "Contents" / "MacOS" / "marker").read_text(), "current")

    def test_a_legacy_link_is_left_alone(self) -> None:
        self.make_bundle(self.app, BUNDLE_ID)
        self.legacy_app.symlink_to("Jotra.app")
        result = self.run_hook("preinstall")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.legacy_app.readlink(), pathlib.Path("Jotra.app"))
        self.assertTrue(self.app.is_dir())

    def test_a_legacy_named_bundle_of_another_app_is_left_alone(self) -> None:
        self.make_bundle(self.legacy_app, "com.example.other")
        result = self.run_hook("preinstall")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.legacy_app / "Contents" / "MacOS" / "marker").read_text(), "com.example.other")
        self.assertFalse(self.app.exists())

    def test_postinstall_links_and_hides_the_legacy_name_then_opens_the_app(self) -> None:
        self.make_bundle(self.app, BUNDLE_ID)
        result = self.run_hook("postinstall")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.legacy_app.readlink(), pathlib.Path("Jotra.app"))
        self.assertEqual(self.hook_calls("chflags"), [["-h", "hidden", str(self.legacy_app)]])
        self.assertEqual(self.hook_calls("sudo"), [["-u", "alice", "open", "-a", str(self.app)]])

    def test_postinstall_keeps_whatever_holds_the_legacy_name(self) -> None:
        self.make_bundle(self.app, BUNDLE_ID)
        self.make_bundle(self.legacy_app, "com.example.other")
        result = self.run_hook("postinstall")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.legacy_app.is_symlink())
        self.assertEqual(self.hook_calls("chflags"), [])

    def test_postinstall_opens_nothing_without_a_console_user(self) -> None:
        self.make_bundle(self.app, BUNDLE_ID)
        result = self.run_hook("postinstall", FAKE_CONSOLE_USER="root")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.hook_calls("sudo"), [])


if __name__ == "__main__":
    unittest.main()
