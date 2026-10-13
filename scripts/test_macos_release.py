"""Tests for scripts/macos-release.sh.

Stubbed Apple tools check the order and flags of each step on any OS. On macOS
the disk-image tests also run hdiutil and codesign for real.

Run from the repository root:
    python3 -m unittest discover -s scripts -p 'test_*.py'
"""

from __future__ import annotations

import os
import pathlib
import platform
import plistlib
import shutil
import subprocess
import tempfile
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "macos-release.sh"
IDENTITY = "Developer ID Application: Example (TEAMID1234)"
ON_MAC = platform.system() == "Darwin" and shutil.which("hdiutil") is not None

# Each stub records its argv as one NUL-separated file per call.
RECORDER = """#!/bin/sh
count=$(ls "$CALL_LOG" | wc -l | tr -d ' ')
{ printf '%s\\0' "$(basename "$0")"; printf '%s\\0' "$@"; } > "$CALL_LOG/$(printf '%04d' "$count")"
"""

CODESIGN_STUB = RECORDER + """
case "$1" in
  -dvvv)
    printf '%s\\n' "${CODESIGN_DETAILS:-CodeDirectory v=20500 size=1 flags=0x10000(runtime) hashes=1+1}" >&2
    if [ -z "${CODESIGN_NO_TIMESTAMP:-}" ]; then echo "Timestamp=1 Jan 2026 at 00:00:00" >&2; fi
    echo "CDHash=${CODESIGN_CDHASH:-0123abcd}" >&2
    ;;
esac
exit 0
"""

XCRUN_STUB = RECORDER + """
if [ "$1" = "notarytool" ] && [ "$2" = "submit" ]; then
  printf '{"id":"submission-1","status":"%s","message":"done"}\\n' "${NOTARY_STATUS:-Accepted}"
fi
exit 0
"""

PLAIN_STUB = RECORDER + "exit 0\n"


def write_app(root: pathlib.Path, name: str = "Writ.app", executable: str = "writ-tauri") -> pathlib.Path:
    app = root / name
    (app / "Contents" / "MacOS").mkdir(parents=True)
    (app / "Contents" / "Resources").mkdir()
    with (app / "Contents" / "Info.plist").open("wb") as handle:
        plistlib.dump({"CFBundleExecutable": executable, "CFBundleIdentifier": "com.writ.editor"}, handle)
    for binary in (executable, "writ"):
        path = app / "Contents" / "MacOS" / binary
        path.write_bytes(b"\xcf\xfa\xed\xfe")
        path.chmod(0o755)
    (app / "Contents" / "Resources" / "THIRD-PARTY-NOTICES.md").write_text("notices\n")
    return app


class StubbedToolsTest(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="writ-macos-release-test-")
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.stubs = self.root / "stubs"
        self.stubs.mkdir()
        self.log = self.root / "calls"
        self.log.mkdir()
        for name, body in (
            ("codesign", CODESIGN_STUB),
            ("xcrun", XCRUN_STUB),
            ("xattr", PLAIN_STUB),
            ("ditto", PLAIN_STUB),
            ("hdiutil", PLAIN_STUB),
        ):
            stub = self.stubs / name
            stub.write_text(body)
            stub.chmod(0o755)
        self.env = {
            "PATH": f"{self.stubs}:{os.environ.get('PATH', '/usr/bin:/bin')}",
            "HOME": str(self.root),
            "TMPDIR": str(self.root),
            "CALL_LOG": str(self.log),
        }
        for tool in ("codesign", "xcrun", "xattr", "ditto", "hdiutil"):
            self.assertEqual(shutil.which(tool, path=self.env["PATH"]), str(self.stubs / tool))

    def run_script(self, *args: str, **env: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(SCRIPT), *args],
            env={**self.env, **env},
            capture_output=True,
            text=True,
            check=False,
        )

    def calls(self) -> list[list[str]]:
        return [record.read_bytes().decode().split("\0")[:-1] for record in sorted(self.log.iterdir())]


class SignAppTests(StubbedToolsTest):
    def test_nested_code_is_signed_before_the_bundle_with_runtime_and_timestamp(self) -> None:
        app = write_app(self.root)
        result = self.run_script("sign-app", str(app), IDENTITY, "/tmp/build.keychain-db")
        self.assertEqual(result.returncode, 0, result.stderr)

        flags = ["--force", "--sign", IDENTITY, "--options", "runtime", "--timestamp", "--keychain", "/tmp/build.keychain-db"]
        signing = [call for call in self.calls() if call[0] == "codesign" and "--sign" in call]
        self.assertEqual(
            signing,
            [
                ["codesign", *flags, str(app / "Contents" / "MacOS" / "writ")],
                ["codesign", *flags, str(app)],
            ],
        )
        self.assertIn(["xattr", "-cr", str(app)], self.calls())
        self.assertIn(["codesign", "--verify", "--strict", "--deep", "--verbose=2", str(app)], self.calls())

    def test_the_main_executable_is_sealed_by_the_bundle_alone(self) -> None:
        app = write_app(self.root)
        self.run_script("sign-app", str(app), IDENTITY)
        signed = [call[-1] for call in self.calls() if call[0] == "codesign" and "--sign" in call]
        self.assertNotIn(str(app / "Contents" / "MacOS" / "writ-tauri"), signed)

    def test_frameworks_are_signed_before_the_bundle(self) -> None:
        app = write_app(self.root)
        framework = app / "Contents" / "Frameworks" / "Example.framework"
        framework.mkdir(parents=True)
        self.run_script("sign-app", str(app), IDENTITY)
        signed = [call[-1] for call in self.calls() if call[0] == "codesign" and "--sign" in call]
        self.assertEqual(signed.index(str(framework)), 0)
        self.assertEqual(signed[-1], str(app))

    def test_a_signature_without_the_hardened_runtime_fails(self) -> None:
        app = write_app(self.root)
        result = self.run_script(
            "sign-app", str(app), IDENTITY, CODESIGN_DETAILS="CodeDirectory v=20500 size=1 flags=0x0(none)"
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("hardened runtime", result.stderr)

    def test_a_developer_id_signature_without_a_timestamp_fails(self) -> None:
        app = write_app(self.root)
        result = self.run_script("sign-app", str(app), IDENTITY, CODESIGN_NO_TIMESTAMP="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("secure timestamp", result.stderr)

    def test_an_ad_hoc_signature_needs_no_timestamp(self) -> None:
        app = write_app(self.root)
        result = self.run_script("sign-app", str(app), "-", CODESIGN_NO_TIMESTAMP="1")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_a_folder_that_is_not_a_bundle_is_refused(self) -> None:
        result = self.run_script("sign-app", str(self.root), IDENTITY)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not an app bundle", result.stderr)
        self.assertEqual(self.calls(), [])


@unittest.skipUnless(shutil.which("jq"), "jq is not installed")
class NotarizeTests(StubbedToolsTest):
    CREDENTIALS = {"APPLE_ID": "dev@example.com", "APPLE_PASSWORD": "app-specific", "APPLE_TEAM_ID": "TEAMID1234"}

    def test_an_app_is_zipped_submitted_then_stapled_in_place(self) -> None:
        app = write_app(self.root)
        result = self.run_script("notarize", str(app), **self.CREDENTIALS)
        self.assertEqual(result.returncode, 0, result.stderr)

        calls = self.calls()
        zipped = next(call for call in calls if call[0] == "ditto")
        self.assertEqual(zipped[1:5], ["-c", "-k", "--keepParent", str(app)])
        submit = next(call for call in calls if call[:3] == ["xcrun", "notarytool", "submit"])
        self.assertEqual(submit[3], zipped[5])
        self.assertIn("--wait", submit)
        self.assertEqual(calls[-2:], [["xcrun", "stapler", "staple", str(app)], ["xcrun", "stapler", "validate", str(app)]])

    def test_a_pkg_is_submitted_as_it_is(self) -> None:
        pkg = self.root / "Writ_0.6.0_universal.pkg"
        pkg.write_bytes(b"pkg")
        result = self.run_script("notarize", str(pkg), **self.CREDENTIALS)
        self.assertEqual(result.returncode, 0, result.stderr)
        submit = next(call for call in self.calls() if call[:3] == ["xcrun", "notarytool", "submit"])
        self.assertEqual(submit[3], str(pkg))
        self.assertFalse(any(call[0] == "ditto" for call in self.calls()))

    def test_a_rejected_submission_fetches_the_log_and_staples_nothing(self) -> None:
        pkg = self.root / "Writ_0.6.0_universal.pkg"
        pkg.write_bytes(b"pkg")
        result = self.run_script("notarize", str(pkg), NOTARY_STATUS="Invalid", **self.CREDENTIALS)
        self.assertEqual(result.returncode, 1)
        self.assertIn("came back Invalid", result.stderr)
        calls = self.calls()
        self.assertTrue(any(call[:4] == ["xcrun", "notarytool", "log", "submission-1"] for call in calls))
        self.assertFalse(any(call[:2] == ["xcrun", "stapler"] for call in calls))

    def test_missing_credentials_stop_before_any_submission(self) -> None:
        pkg = self.root / "Writ_0.6.0_universal.pkg"
        pkg.write_bytes(b"pkg")
        result = self.run_script("notarize", str(pkg), APPLE_ID="dev@example.com")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("APPLE_PASSWORD", result.stderr)
        self.assertEqual(self.calls(), [])


class RepackStubTests(StubbedToolsTest):
    def test_an_image_without_the_app_is_refused_and_detached(self) -> None:
        built = self.root / "built.dmg"
        built.write_bytes(b"dmg")
        app = write_app(self.root / "signed")
        out = self.root / "out.dmg"
        result = self.run_script("repack-dmg", str(built), str(app), str(out))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("holds no Writ.app", result.stderr)
        verbs = [call[1] for call in self.calls() if call[0] == "hdiutil"]
        self.assertEqual(verbs[:2], ["convert", "attach"])
        self.assertIn("detach", verbs)

    def test_an_existing_output_is_never_overwritten(self) -> None:
        built = self.root / "built.dmg"
        built.write_bytes(b"dmg")
        app = write_app(self.root / "signed")
        out = self.root / "out.dmg"
        out.write_bytes(b"keep")
        result = self.run_script("repack-dmg", str(built), str(app), str(out))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(out.read_bytes(), b"keep")
        self.assertEqual(self.calls(), [])

    def test_an_unknown_command_prints_the_choices(self) -> None:
        result = self.run_script("sign-everything")
        self.assertEqual(result.returncode, 1)
        self.assertIn("extract-app, sign-app, notarize or repack-dmg", result.stderr)


@unittest.skipUnless(ON_MAC, "needs macOS with hdiutil and codesign")
class DiskImageTests(unittest.TestCase):
    """Real hdiutil and codesign runs on a bundle built from /usr/bin/true."""

    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="writ-dmg-test-")
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.env = {**os.environ, "TMPDIR": str(self.root)}

    def bundle(self, folder: str, marker: str) -> pathlib.Path:
        app = self.root / folder / "Writ.app"
        (app / "Contents" / "MacOS").mkdir(parents=True)
        (app / "Contents" / "Resources").mkdir()
        for name in ("writ-tauri", "writ"):
            executable = app / "Contents" / "MacOS" / name
            shutil.copyfile("/usr/bin/true", executable)
            executable.chmod(0o755)
        with (app / "Contents" / "Info.plist").open("wb") as handle:
            plistlib.dump(
                {
                    "CFBundleExecutable": "writ-tauri",
                    "CFBundleIdentifier": "com.writ.editor.test",
                    "CFBundlePackageType": "APPL",
                },
                handle,
            )
        (app / "Contents" / "Resources" / "marker.txt").write_text(marker)
        return app

    def run_script(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(SCRIPT), *args], env=self.env, capture_output=True, text=True, check=False
        )

    def cdhash(self, path: pathlib.Path) -> str:
        details = subprocess.run(["codesign", "-dvvv", str(path)], capture_output=True, text=True, check=True).stderr
        return next(line.split("=", 1)[1] for line in details.splitlines() if line.startswith("CDHash="))

    def built_image(self) -> pathlib.Path:
        """A UDZO image laid out like the bundler's: the app, an Applications link and a volume icon.

        macOS drops a .DS_Store it cannot parse, so the fixture carries other
        Finder files and the test requires every entry outside the app to come
        through unchanged.
        """
        source = self.root / "image-root"
        source.mkdir()
        shutil.copytree(self.bundle("unsigned", "build"), source / "Writ.app", symlinks=True)
        (source / "Applications").symlink_to("/Applications")
        (source / ".VolumeIcon.icns").write_bytes(b"icns")
        (source / ".background").mkdir()
        (source / ".background" / "backdrop.png").write_bytes(b"png")
        image = self.root / "Writ_0.6.0_universal.dmg"
        subprocess.run(
            ["hdiutil", "create", "-quiet", "-volname", "Writ", "-srcfolder", str(source), "-format", "UDZO", str(image)],
            check=True,
        )
        return image

    def outside_the_app(self, mountpoint: pathlib.Path) -> dict[str, str]:
        entries: dict[str, str] = {}
        for path in sorted(mountpoint.rglob("*")):
            relative = path.relative_to(mountpoint)
            if relative.parts[0] in ("Writ.app", ".fseventsd", ".Trashes", ".Spotlight-V100"):
                continue
            if path.is_symlink():
                entries[str(relative)] = f"link:{os.readlink(path)}"
            elif path.is_dir():
                entries[str(relative)] = "dir"
            else:
                entries[str(relative)] = path.read_bytes().hex()
        return entries

    def mounted(self, image: pathlib.Path) -> pathlib.Path:
        mountpoint = pathlib.Path(tempfile.mkdtemp(prefix="mnt-", dir=self.root))
        subprocess.run(
            ["hdiutil", "attach", str(image), "-readonly", "-nobrowse", "-noautoopen", "-mountpoint", str(mountpoint), "-quiet"],
            check=True,
        )
        self.addCleanup(subprocess.run, ["hdiutil", "detach", str(mountpoint), "-force", "-quiet"], check=False)
        return mountpoint

    def test_sign_app_signs_a_real_bundle_with_the_hardened_runtime(self) -> None:
        app = self.bundle("to-sign", "signed")
        result = self.run_script("sign-app", str(app), "-")
        self.assertEqual(result.returncode, 0, result.stderr)
        details = subprocess.run(["codesign", "-dvvv", str(app)], capture_output=True, text=True, check=True).stderr
        self.assertIn("runtime", details)
        subprocess.run(["codesign", "--verify", "--strict", "--deep", str(app)], check=True)

    def test_extract_app_copies_the_bundle_out_of_the_image(self) -> None:
        image = self.built_image()
        dest = self.root / "extracted" / "Writ.app"
        dest.parent.mkdir()
        result = self.run_script("extract-app", str(image), str(dest))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((dest / "Contents" / "Resources" / "marker.txt").read_text(), "build")
        self.assertTrue(os.access(dest / "Contents" / "MacOS" / "writ-tauri", os.X_OK))

    def test_repack_swaps_in_the_signed_bundle_and_keeps_the_layout(self) -> None:
        image = self.built_image()
        signed = self.bundle("signed", "signed")
        sign = self.run_script("sign-app", str(signed), "-")
        self.assertEqual(sign.returncode, 0, sign.stderr)
        out = self.root / "out" / "Writ_0.6.0_universal.dmg"
        out.parent.mkdir()

        result = self.run_script("repack-dmg", str(image), str(signed), str(out))
        self.assertEqual(result.returncode, 0, result.stderr)

        before = self.outside_the_app(self.mounted(image))
        mountpoint = self.mounted(out)
        self.assertEqual((mountpoint / "Writ.app" / "Contents" / "Resources" / "marker.txt").read_text(), "signed")
        self.assertEqual(self.cdhash(mountpoint / "Writ.app"), self.cdhash(signed))
        self.assertEqual(self.outside_the_app(mountpoint), before)
        self.assertEqual(before["Applications"], "link:/Applications")
        self.assertIn(".VolumeIcon.icns", before)
        self.assertFalse((mountpoint / ".fseventsd").exists())
        info = subprocess.run(["hdiutil", "imageinfo", str(out)], capture_output=True, text=True, check=True).stdout
        self.assertIn("UDZO", info)


if __name__ == "__main__":
    unittest.main()
