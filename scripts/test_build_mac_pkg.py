"""Tests for scripts/build-mac-pkg.sh with stubbed macOS packaging tools.

Run from the repository root:
    python3 -m unittest discover -s scripts -p 'test_*.py'
"""

from __future__ import annotations

import os
import pathlib
import plistlib
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
scripts=""
component_plist=""
while [ "$#" -gt 1 ]; do
  case "$1" in
    --root) root="$2" ;;
    --scripts) scripts="$2" ;;
    --component-plist) component_plist="$2" ;;
  esac
  shift
done
ls "$root/Applications" > "$CALL_LOG/../pkgbuild-applications.txt"
cp -R "$scripts" "$CALL_LOG/../pkgbuild-scripts"
cp "$component_plist" "$CALL_LOG/../pkgbuild-component.plist"
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


class MacPkgFixture(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="jotra-mac-pkg-test-")
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
        app = parent / "Jotra.app"
        (app / "Contents" / "MacOS").mkdir(parents=True)
        (app / "Contents" / "MacOS" / "jotra-tauri").write_text("binary")
        return app

    def run_script(self, **env: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(SCRIPT)], env={**self.env, **env}, capture_output=True, text=True, check=False
        )

    def calls(self, tool: str) -> list[list[str]]:
        records = [record.read_bytes().decode().split("\0")[:-1] for record in sorted(self.log.iterdir())]
        return [record[1:] for record in records if record[0] == tool]

    def build_package(self) -> pathlib.Path:
        app = self.make_app(self.root / "signed")
        result = self.run_script(JOTRA_APP_PATH=str(app), JOTRA_PKG_OUT_DIR=str(self.root / "out"))
        self.assertEqual(result.returncode, 0, result.stderr)
        return self.root


class BuildMacPkgTests(MacPkgFixture):
    def test_a_named_app_is_packaged_into_a_named_folder_without_cargo(self) -> None:
        app = self.make_app(self.root / "signed")
        out = self.root / "dist" / "signed-macos"
        result = self.run_script(JOTRA_APP_PATH=str(app), JOTRA_PKG_OUT_DIR=str(out))
        self.assertEqual(result.returncode, 0, result.stderr)

        pkg = out / f"Jotra_{VERSION}_universal.pkg"
        self.assertEqual(pkg.read_text(), "unsigned installer\n")
        self.assertEqual(self.calls("cargo"), [])
        [pkgbuild] = self.calls("pkgbuild")
        self.assertEqual(pkgbuild[pkgbuild.index("--identifier") + 1], "com.writ.editor")
        self.assertEqual(pkgbuild[pkgbuild.index("--version") + 1], VERSION)
        self.assertEqual((self.root / "pkgbuild-applications.txt").read_text(), "Jotra.app\n")
        self.assertEqual(self.calls("productsign"), [])
        self.assertIn("installer is unsigned", result.stderr)

    def test_the_installer_identity_signs_the_named_output(self) -> None:
        app = self.make_app(self.root / "signed")
        out = self.root / "out"
        identity = "Developer ID Installer: Example (TEAMID1234)"
        result = self.run_script(
            JOTRA_APP_PATH=str(app), JOTRA_PKG_OUT_DIR=str(out), JOTRA_INSTALLER_SIGNING_IDENTITY=identity
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        [productsign] = self.calls("productsign")
        self.assertEqual(productsign[:2], ["--sign", identity])
        self.assertEqual(productsign[-1], str(out / f"Jotra_{VERSION}_universal.pkg"))
        self.assertEqual(self.calls("pkgutil"), [["--check-signature", str(out / f"Jotra_{VERSION}_universal.pkg")]])

    def test_without_overrides_the_app_comes_from_the_cargo_target_dir(self) -> None:
        bundle_dir = self.root / "target" / "universal-apple-darwin" / "release" / "bundle" / "macos"
        self.make_app(bundle_dir)
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls("cargo")[0][:1], ["metadata"])
        self.assertTrue((bundle_dir / f"Jotra_{VERSION}_universal.pkg").is_file())

    def test_a_missing_app_stops_before_packaging(self) -> None:
        result = self.run_script(JOTRA_APP_PATH=str(self.root / "nowhere" / "Jotra.app"), JOTRA_PKG_OUT_DIR=str(self.root))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no .app at", result.stderr)
        self.assertEqual(self.calls("pkgbuild"), [])

    def test_the_component_plist_installs_the_bundle_in_place(self) -> None:
        packaged = self.build_package()
        [pkgbuild] = self.calls("pkgbuild")
        self.assertIn("--component-plist", pkgbuild)
        with (packaged / "pkgbuild-component.plist").open("rb") as handle:
            [component] = plistlib.load(handle)
        self.assertEqual(component["RootRelativeBundlePath"], "Applications/Jotra.app")
        self.assertIs(component["BundleIsRelocatable"], False)
        self.assertEqual(component["BundleOverwriteAction"], "upgrade")

    def test_the_install_scripts_are_bash_that_stops_on_errors(self) -> None:
        scripts = self.build_package() / "pkgbuild-scripts"
        self.assertEqual(sorted(path.name for path in scripts.iterdir()), ["postinstall", "preinstall"])
        for script in scripts.iterdir():
            with self.subTest(script=script.name):
                self.assertTrue(os.access(script, os.X_OK))
                text = script.read_text()
                self.assertTrue(text.startswith("#!/bin/bash\nset -euo pipefail\n"), text[:40])
                checked = subprocess.run(["bash", "-n", str(script)], capture_output=True, text=True, check=False)
                self.assertEqual(checked.returncode, 0, checked.stderr)


if __name__ == "__main__":
    unittest.main()
