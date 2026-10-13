"""Tests for scripts/build-mac-pkg.sh with stubbed macOS packaging tools.

Run from the repository root:
    python3 -m unittest discover -s scripts -p 'test_*.py'
"""

from __future__ import annotations

import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "build-mac-pkg.sh"
VERSION = re.search(r'^version\s*=\s*"([^"]+)"', (REPO_ROOT / "Cargo.toml").read_text(), re.MULTILINE).group(1)

RECORDER = """#!/bin/sh
count=$(ls "$CALL_LOG" | wc -l | tr -d ' ')
{ printf '%s\\0' "$(basename "$0")"; printf '%s\\0' "$@"; } > "$CALL_LOG/$(printf '%04d' "$count")"
"""

STUBS = {
    # The component package records which app it was handed.
    "pkgbuild": RECORDER
    + """
root=""
while [ "$#" -gt 1 ]; do
  if [ "$1" = "--root" ]; then root="$2"; fi
  shift
done
ls "$root/Applications" > "$CALL_LOG/../pkgbuild-applications.txt"
printf 'component\n' > "$1"
""",
    "productbuild": RECORDER
    + """
for last; do :; done
printf 'unsigned installer\\n' > "$last"
""",
    "productsign": RECORDER + 'cp "$3" "$4"\n',
    "pkgutil": RECORDER + "exit 0\n",
    "cargo": RECORDER
    + """
printf '{"target_directory": "%s"}\\n' "$FAKE_TARGET_DIR"
""",
}


class BuildMacPkgTests(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="writ-mac-pkg-test-")
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        stubs = self.root / "stubs"
        stubs.mkdir()
        for name, body in STUBS.items():
            stub = stubs / name
            stub.write_text(body)
            stub.chmod(0o755)
        (stubs / "python3").symlink_to(sys.executable)
        self.log = self.root / "calls"
        self.log.mkdir()
        self.env = {
            "PATH": f"{stubs}:/usr/bin:/bin",
            "HOME": str(self.root),
            "TMPDIR": str(self.root),
            "CALL_LOG": str(self.log),
            "FAKE_TARGET_DIR": str(self.root / "target"),
        }
        for name in STUBS:
            self.assertEqual(shutil.which(name, path=self.env["PATH"]), str(stubs / name))

    def make_app(self, parent: pathlib.Path) -> pathlib.Path:
        app = parent / "Writ.app"
        (app / "Contents" / "MacOS").mkdir(parents=True)
        (app / "Contents" / "MacOS" / "writ-tauri").write_text("binary")
        return app

    def run_script(self, **env: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(SCRIPT)], env={**self.env, **env}, capture_output=True, text=True, check=False
        )

    def calls(self, tool: str) -> list[list[str]]:
        records = [record.read_bytes().decode().split("\0")[:-1] for record in sorted(self.log.iterdir())]
        return [record[1:] for record in records if record[0] == tool]

    def test_a_named_app_is_packaged_into_a_named_folder_without_cargo(self) -> None:
        app = self.make_app(self.root / "signed")
        out = self.root / "dist" / "signed-macos"
        result = self.run_script(WRIT_APP_PATH=str(app), WRIT_PKG_OUT_DIR=str(out))
        self.assertEqual(result.returncode, 0, result.stderr)

        pkg = out / f"Writ_{VERSION}_universal.pkg"
        self.assertEqual(pkg.read_text(), "unsigned installer\n")
        self.assertEqual(self.calls("cargo"), [])
        [pkgbuild] = self.calls("pkgbuild")
        self.assertEqual(pkgbuild[pkgbuild.index("--identifier") + 1], "com.writ.editor")
        self.assertEqual(pkgbuild[pkgbuild.index("--version") + 1], VERSION)
        self.assertEqual((self.root / "pkgbuild-applications.txt").read_text(), "Writ.app\n")
        self.assertEqual(self.calls("productsign"), [])
        self.assertIn("installer is unsigned", result.stderr)

    def test_the_installer_identity_signs_the_named_output(self) -> None:
        app = self.make_app(self.root / "signed")
        out = self.root / "out"
        identity = "Developer ID Installer: Example (TEAMID1234)"
        result = self.run_script(
            WRIT_APP_PATH=str(app), WRIT_PKG_OUT_DIR=str(out), WRIT_INSTALLER_SIGNING_IDENTITY=identity
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        [productsign] = self.calls("productsign")
        self.assertEqual(productsign[:2], ["--sign", identity])
        self.assertEqual(productsign[-1], str(out / f"Writ_{VERSION}_universal.pkg"))
        self.assertEqual(self.calls("pkgutil"), [["--check-signature", str(out / f"Writ_{VERSION}_universal.pkg")]])

    def test_without_overrides_the_app_comes_from_the_cargo_target_dir(self) -> None:
        bundle_dir = self.root / "target" / "universal-apple-darwin" / "release" / "bundle" / "macos"
        self.make_app(bundle_dir)
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls("cargo")[0][:1], ["metadata"])
        self.assertTrue((bundle_dir / f"Writ_{VERSION}_universal.pkg").is_file())

    def test_a_missing_app_stops_before_packaging(self) -> None:
        result = self.run_script(WRIT_APP_PATH=str(self.root / "nowhere" / "Writ.app"), WRIT_PKG_OUT_DIR=str(self.root))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no .app at", result.stderr)
        self.assertEqual(self.calls("pkgbuild"), [])


if __name__ == "__main__":
    unittest.main()
