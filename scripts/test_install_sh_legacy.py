"""Tests for how install.sh treats installs made under the old product name.

Run from the repository root:
    python3 -m unittest discover -s scripts -p 'test_*.py'
"""

from __future__ import annotations

import importlib.util
import pathlib
import unittest

FIXTURE_PATH = pathlib.Path(__file__).with_name("test_install_sh.py")
SPEC = importlib.util.spec_from_file_location("test_install_sh_fixture", FIXTURE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Could not load {FIXTURE_PATH}")
FIXTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURE)

def legacy_command_name(command: str) -> str | None:
    return "writ" if command == "writ" else None


OLD_WRAPPER = '#!/bin/sh\nWRIT_GUI_BIN="{gui}"; export WRIT_GUI_BIN\nexec "{cli}" "$@"\n'


@unittest.skipIf(FIXTURE.OPENSSL is None, "signing the fixtures needs OpenSSL 3.0 or later")
class LegacyInstallScriptTests(FIXTURE.InstallScriptFixture):
    def make_old_layout(self) -> pathlib.Path:
        layout = self.home / ".local/share/writ"
        layout.mkdir(parents=True)
        (layout / "Writ.AppImage").write_text("old image\n")
        (layout / "writ-cli").write_text("#!/bin/sh\necho old\n")
        (layout / "writ-gui").write_text("old launcher\n")
        bin_dir = self.home / ".local/bin"
        bin_dir.mkdir(parents=True)
        (bin_dir / "writ").write_text(OLD_WRAPPER.format(gui=layout / "writ-gui", cli=layout / "writ-cli"))
        return layout

    def test_a_fresh_install_also_installs_the_old_command(self) -> None:
        self.use("openssl", FIXTURE.OPENSSL)
        self.stub("fusermount", FIXTURE.FUSERMOUNT)

        self.assert_installed(self.run_install())

        layout = self.home / ".local/share/jotra"
        self.assert_wrapper("writ", layout / "Jotra.AppImage", layout / "jotra-cli", command_name="writ")
        self.assert_wrapper("jotra", layout / "Jotra.AppImage", layout / "jotra-cli")
        self.assertFalse((self.home / ".local/share/writ").exists())

    def test_an_old_layout_is_updated_in_place(self) -> None:
        self.use("openssl", FIXTURE.OPENSSL)
        self.stub("fusermount", FIXTURE.FUSERMOUNT)
        layout = self.make_old_layout()

        result = self.run_install()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f"installed {FIXTURE.VERSION}", result.stdout)
        self.assertIn(f"app:     {layout / 'Writ.AppImage'}", result.stdout)
        self.assertEqual((layout / "Writ.AppImage").read_bytes(), FIXTURE.FAKE_APPIMAGE.encode())
        self.assertEqual((layout / "writ-cli").read_text(), "#!/bin/sh\necho jotra\n")
        self.assertFalse((self.home / ".local/share/jotra").exists())
        for command in ("writ", "jotra"):
            self.assert_wrapper(command, layout / "Writ.AppImage", layout / "writ-cli", legacy_command_name(command))

    def test_an_old_layout_without_fusermount_keeps_its_unpacked_launcher(self) -> None:
        self.use("openssl", FIXTURE.OPENSSL)
        layout = self.make_old_layout()

        result = self.run_install()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            (layout / "writ-gui").read_text(),
            f'#!/bin/sh\nexec "{layout / "Writ.AppImage"}" --appimage-extract-and-run "$@"\n',
        )
        self.assertFalse((self.home / ".local/share/jotra").exists())
        for command in ("writ", "jotra"):
            self.assert_wrapper(command, layout / "writ-gui", layout / "writ-cli", legacy_command_name(command))

    def test_the_old_command_keeps_a_gui_path_the_person_set(self) -> None:
        self.use("openssl", FIXTURE.OPENSSL)
        self.stub("fusermount", FIXTURE.FUSERMOUNT)

        self.assert_installed(self.run_install())

        layout = self.home / ".local/share/jotra"
        self.assert_gui_variable_precedence("writ", layout / "Jotra.AppImage", layout / "jotra-cli")

    def test_an_old_layout_keeps_a_gui_path_the_person_set(self) -> None:
        self.use("openssl", FIXTURE.OPENSSL)
        self.stub("fusermount", FIXTURE.FUSERMOUNT)
        layout = self.make_old_layout()

        result = self.run_install()

        self.assertEqual(result.returncode, 0, result.stderr)
        for command in ("writ", "jotra"):
            self.assert_gui_variable_precedence(command, layout / "Writ.AppImage", layout / "writ-cli")

    def test_the_old_escape_variable_still_skips_the_signature_check(self) -> None:
        result = self.run_install(WRIT_INSTALL_SKIP_SIGNATURE="1")

        self.assert_installed(result)
        self.assertIn("WRIT_INSTALL_SKIP_SIGNATURE=1 is set, so the release signature is not checked", result.stderr)
        self.assertNotIn("SHA256SUMS.txt.minisig", self.downloads())

    def test_the_old_escape_variable_still_checks_the_checksum(self) -> None:
        (self.fixtures / FIXTURE.ASSET).write_bytes(FIXTURE.FAKE_APPIMAGE.encode() + b"# tampered\n")

        self.assert_refused(self.run_install(WRIT_INSTALL_SKIP_SIGNATURE="1"), "checksum mismatch")


if __name__ == "__main__":
    unittest.main()
