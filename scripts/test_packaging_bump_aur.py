import base64
import importlib.util
import os
import pathlib
import platform
import re
import shutil
import subprocess
import tempfile
import unittest


SCRIPT_PATH = pathlib.Path(__file__).with_name("packaging_bump_aur.py")
SPEC = importlib.util.spec_from_file_location("packaging_bump_aur", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Could not load {SCRIPT_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)

ROOT = SCRIPT_PATH.parent.parent
REPO_PKGBUILD = ROOT / "packaging/aur/writ-bin/PKGBUILD"
SHA = "a" * 64
LICENSE_SHA = "b" * 64
BASH = shutil.which("bash")


class RewritePkgbuildTests(unittest.TestCase):
    def test_rewrites_version_checksum_and_description(self) -> None:
        stale = REPO_PKGBUILD.read_text().replace(
            f'pkgdesc="{MODULE.PACKAGE_DESCRIPTION}"', 'pkgdesc="Old scratchpad copy"'
        )

        result = MODULE.rewrite_pkgbuild(stale, "9.9.9", SHA, LICENSE_SHA)

        self.assertIn("pkgver=9.9.9\n", result)
        self.assertIn("pkgrel=1\n", result)
        self.assertIn(f'pkgdesc="{MODULE.PACKAGE_DESCRIPTION}"\n', result)
        self.assertIn(f"sha256sums=('{SHA}'\n            'SKIP'\n            '{LICENSE_SHA}')", result)
        self.assertNotIn("scratchpad", result)

    def test_pins_the_licence_and_skips_only_the_signature(self) -> None:
        result = MODULE.rewrite_pkgbuild(REPO_PKGBUILD.read_text(), "9.9.9", SHA, LICENSE_SHA)

        sums = re.search(r"sha256sums=\(([^)]*)\)", result).group(1).split()  # type: ignore[union-attr]
        sources = re.search(r"source=\(([^)]*)\)", result).group(1).split()  # type: ignore[union-attr]
        self.assertEqual(len(sums), len(sources))
        self.assertEqual(sums, [f"'{SHA}'", "'SKIP'", f"'{LICENSE_SHA}'"])
        self.assertIn(".AppImage.minisign::", sources[1])
        self.assertTrue(sources[2].startswith('"LICENSE-'))

    def test_the_srcinfo_lists_what_the_pkgbuild_does(self) -> None:
        text = MODULE.srcinfo_text("9.9.9", SHA, LICENSE_SHA)

        self.assertIn("\tmakedepends = minisign\n", text)
        self.assertIn(
            "\tsource = writ-9.9.9.AppImage.minisign::"
            "https://github.com/ibrahemid/writ/releases/download/v9.9.9/Writ_9.9.9_amd64.AppImage.sig\n",
            text,
        )
        self.assertEqual(
            re.findall(r"\tsha256sums = (\S+)", text), [SHA, "SKIP", LICENSE_SHA]
        )

    def test_refuses_a_pkgbuild_without_a_description(self) -> None:
        text = "pkgver=1.0.0\npkgrel=2\nsha256sums=('x'\n 'SKIP')\n"
        with self.assertRaisesRegex(MODULE.PkgbuildFormatError, "pkgdesc"):
            MODULE.rewrite_pkgbuild(text, "1.0.1", SHA, LICENSE_SHA)

    def test_description_matches_the_committed_manifests(self) -> None:
        root = SCRIPT_PATH.parent.parent
        self.assertIn(f'pkgdesc="{MODULE.PACKAGE_DESCRIPTION}"', REPO_PKGBUILD.read_text())
        self.assertIn(
            f"pkgdesc = {MODULE.PACKAGE_DESCRIPTION}\n",
            (root / "packaging/aur/writ-bin/.SRCINFO").read_text(),
        )


    def test_the_committed_srcinfo_matches_the_pkgbuild(self) -> None:
        pkgbuild = REPO_PKGBUILD.read_text()
        version = re.search(r"^pkgver=(\S+)$", pkgbuild, re.MULTILINE).group(1)  # type: ignore[union-attr]
        sums = re.findall(r"'([0-9a-f]{64}|SKIP)'", re.search(r"sha256sums=\(([^)]*)\)", pkgbuild).group(1))  # type: ignore[union-attr]
        self.assertEqual(sums[1], "SKIP")
        self.assertEqual(
            (ROOT / "packaging/aur/writ-bin/.SRCINFO").read_text(),
            MODULE.srcinfo_text(version, sums[0], sums[2]),
        )


class PkgbuildKeyTests(unittest.TestCase):
    def test_the_pkgbuild_checks_against_the_key_install_sh_trusts(self) -> None:
        pkgbuild_key = re.search(r"^_minisign_pubkey='([^']+)'$", REPO_PKGBUILD.read_text(), re.MULTILINE)
        install_key = re.search(r'^MINISIGN_PUBKEY="([^"]+)"$', (ROOT / "install.sh").read_text(), re.MULTILINE)
        self.assertIsNotNone(pkgbuild_key)
        self.assertIsNotNone(install_key)
        self.assertEqual(pkgbuild_key.group(1), install_key.group(1))  # type: ignore[union-attr]


MINISIGN_STUB = """#!/bin/sh
printf '%s\\n' "$*" > "$MINISIGN_LOG"
exit "$MINISIGN_EXIT"
"""

APPIMAGE_STUB = """#!/bin/sh
mkdir -p squashfs-root
touch squashfs-root/extracted
"""


@unittest.skipIf(BASH is None, "bash is not installed")
class PkgbuildFunctionTests(unittest.TestCase):
    """Runs prepare() and package() from the PKGBUILD the way makepkg does:
    sourced into bash with errexit, in a srcdir holding the downloads."""

    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="writ-pkgbuild-test-")
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.srcdir = self.root / "src"
        self.srcdir.mkdir()
        self.pkgdir = self.root / "pkg"
        self.pkgdir.mkdir()
        stubs = self.root / "stubs"
        stubs.mkdir()
        (stubs / "minisign").write_text(MINISIGN_STUB)
        (stubs / "minisign").chmod(0o755)
        self.version = re.search(r"^pkgver=(\S+)$", REPO_PKGBUILD.read_text(), re.MULTILINE).group(1)  # type: ignore[union-attr]
        appimage = self.srcdir / f"writ-{self.version}.AppImage"
        appimage.write_text(APPIMAGE_STUB)
        minisig = "untrusted comment: x\nRUQ=\ntrusted comment: y\nAAAA\n"
        (self.srcdir / f"writ-{self.version}.AppImage.minisign").write_text(
            base64.b64encode(minisig.encode()).decode()
        )
        self.minisign_log = self.root / "minisign.log"
        self.env = {
            **os.environ,
            "PATH": f"{stubs}:{os.environ['PATH']}",
            "MINISIGN_LOG": str(self.minisign_log),
        }

    def run_function(self, name: str, minisign_exit: int = 0) -> subprocess.CompletedProcess[str]:
        command = (
            f'set -e; srcdir="{self.srcdir}"; pkgdir="{self.pkgdir}"; '
            f'source "{REPO_PKGBUILD}"; cd "$srcdir"; {name}'
        )
        return subprocess.run(
            [BASH, "-c", command],
            env={**self.env, "MINISIGN_EXIT": str(minisign_exit)},
            capture_output=True,
            text=True,
        )

    def test_prepare_refuses_an_appimage_whose_signature_fails(self) -> None:
        result = self.run_function("prepare", minisign_exit=1)

        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.srcdir / "squashfs-root").exists())

    def test_prepare_checks_the_appimage_against_the_release_key_before_unpacking(self) -> None:
        result = self.run_function("prepare")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.srcdir / "squashfs-root" / "extracted").exists())
        call = self.minisign_log.read_text()
        key = re.search(r"^_minisign_pubkey='([^']+)'$", REPO_PKGBUILD.read_text(), re.MULTILINE).group(1)  # type: ignore[union-attr]
        self.assertIn(f"-V -P {key}", call)
        self.assertIn(f"-m {self.srcdir}/writ-{self.version}.AppImage", call)
        self.assertTrue((self.srcdir / f"writ-{self.version}.AppImage.minisig").read_text().startswith("untrusted comment:"))

    @unittest.skipUnless(platform.system() == "Linux", "package() uses GNU install and sed")
    def test_package_marks_the_install_as_pacman_managed(self) -> None:
        root = self.srcdir / "squashfs-root"
        (root / "usr/bin").mkdir(parents=True)
        (root / "usr/share/applications").mkdir(parents=True)
        (root / "usr/share/icons/hicolor/32x32/apps").mkdir(parents=True)
        (root / "AppRun").write_text("#!/bin/sh\n")
        (root / "usr/bin/writ").write_text("")
        (root / "usr/share/applications/Writ.desktop").write_text("Exec=x\nIcon=y\n")
        (root / "usr/share/icons/hicolor/32x32/apps/writ-tauri.png").write_text("")
        (self.srcdir / f"LICENSE-{self.version}").write_text("MIT\n")

        result = self.run_function("package")

        self.assertEqual(result.returncode, 0, result.stderr)
        marker = self.pkgdir / "opt/writ/.package-manager"
        self.assertEqual(marker.read_text(), "pacman\n")
        self.assertEqual(marker.stat().st_mode & 0o777, 0o644)


if __name__ == "__main__":
    unittest.main()
