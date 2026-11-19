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
        text = MODULE.srcinfo_text(REPO_PKGBUILD.read_text(), "9.9.9", SHA, LICENSE_SHA)

        self.assertIn("\tmakedepends = minisign\n", text)
        self.assertIn("\tprovides = jotra\n", text)
        self.assertIn("\tconflicts = jotra\n", text)
        self.assertIn(
            "\tsource = jotra-9.9.9.AppImage.minisign::"
            "https://github.com/ibrahemid/writ/releases/download/v9.9.9/Jotra_9.9.9_amd64.AppImage.sig\n",
            text,
        )
        self.assertEqual(
            re.findall(r"\tsha256sums = (\S+)", text), [SHA, "SKIP", LICENSE_SHA]
        )

    def test_the_srcinfo_depends_on_what_the_pkgbuild_does(self) -> None:
        text = MODULE.srcinfo_text(REPO_PKGBUILD.read_text(), "9.9.9", SHA, LICENSE_SHA)
        listed = re.search(r"^depends=\(([^)]*)\)", REPO_PKGBUILD.read_text(), re.MULTILINE)
        assert listed is not None
        pkgbuild_depends = [name.strip("'\"") for name in listed.group(1).split()]

        self.assertEqual(re.findall(r"\tdepends = (\S+)", text), pkgbuild_depends)
        self.assertIn("alsa-lib", pkgbuild_depends)

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
            MODULE.srcinfo_text(pkgbuild, version, sums[0], sums[2]),
        )

    def test_the_srcinfo_expands_the_variables_the_pkgbuild_arrays_name(self) -> None:
        pkgbuild = '_name=alpha\n_other="beta"\nprovides=("${_name}" \'gamma\')\nconflicts=("${_other}")\n'

        text = MODULE.srcinfo_text(pkgbuild, "9.9.9", SHA, LICENSE_SHA)

        self.assertEqual(re.findall(r"\tprovides = (\S+)", text), ["alpha", "gamma"])
        self.assertEqual(re.findall(r"\tconflicts = (\S+)", text), ["beta"])

    def test_the_srcinfo_refuses_an_array_that_names_an_unassigned_variable(self) -> None:
        pkgbuild = 'provides=("${_missing}")\nconflicts=()\n'

        with self.assertRaisesRegex(MODULE.PkgbuildFormatError, "_missing"):
            MODULE.srcinfo_text(pkgbuild, "9.9.9", SHA, LICENSE_SHA)

    def test_the_srcinfo_refuses_a_pkgbuild_without_conflicts(self) -> None:
        with self.assertRaisesRegex(MODULE.PkgbuildFormatError, "conflicts"):
            MODULE.srcinfo_text("provides=('alpha')\n", "9.9.9", SHA, LICENSE_SHA)


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

COMMAND_WRAPPER_BLOCK = re.compile(
    r'^ *cat > "\$\{pkgdir\}/usr/bin/\$\{(?:_pkgname|_legacy_name)\}" <<EOF\n.*?\nEOF$',
    re.MULTILINE | re.DOTALL,
)

GUI_PROBE_CLI = '#!/bin/sh\nprintf "%s\\n" "$JOTRA_GUI_BIN"\n'


class PkgbuildFixture(unittest.TestCase):
    """Runs prepare() and package() from the PKGBUILD the way makepkg does:
    sourced into bash with errexit, in a srcdir holding the downloads."""

    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="jotra-pkgbuild-test-")
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
        appimage = self.srcdir / f"jotra-{self.version}.AppImage"
        appimage.write_text(APPIMAGE_STUB)
        minisig = "untrusted comment: x\nRUQ=\ntrusted comment: y\nAAAA\n"
        (self.srcdir / f"jotra-{self.version}.AppImage.minisign").write_text(
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

    def write_payload(self) -> None:
        root = self.srcdir / "squashfs-root"
        (root / "usr/bin").mkdir(parents=True)
        (root / "usr/share/applications").mkdir(parents=True)
        (root / "usr/share/icons/hicolor/32x32/apps").mkdir(parents=True)
        (root / "AppRun").write_text("#!/bin/sh\n")
        (root / "usr/bin/jotra").write_text("cli\n")
        (root / "usr/share/applications/Jotra.desktop").write_text(
            "[Desktop Entry]\nExec=x\nIcon=y\nMimeType=text/plain;\nStartupWMClass=z\n"
        )
        (root / "usr/share/icons/hicolor/32x32/apps/jotra-tauri.png").write_text("")
        (self.srcdir / f"LICENSE-{self.version}").write_text("MIT\n")

    def write_command_wrappers(self, opt: pathlib.Path) -> None:
        blocks = [match.group(0) for match in COMMAND_WRAPPER_BLOCK.finditer(REPO_PKGBUILD.read_text())]
        self.assertEqual(len(blocks), 2)
        (self.pkgdir / "usr/bin").mkdir(parents=True)
        script = f'set -e; pkgdir="{self.pkgdir}"; source "{REPO_PKGBUILD}"; opt="{opt}"\n' + "\n".join(blocks) + "\n"
        result = subprocess.run([BASH, "-c", script], env=self.env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def assert_gui_variable_precedence(self, command: str) -> None:
        opt = self.root / "opt/jotra"
        (opt / "usr/bin").mkdir(parents=True)
        cli = opt / "usr/bin" / command
        cli.write_text(GUI_PROBE_CLI)
        cli.chmod(0o755)
        self.write_command_wrappers(opt)
        wrapper = self.pkgdir / "usr/bin" / command
        wrapper.chmod(0o755)
        cases = (
            ({}, f"{opt}/AppRun"),
            ({"WRIT_GUI_BIN": "/set/old"}, "/set/old"),
            ({"JOTRA_GUI_BIN": "/set/new"}, "/set/new"),
            ({"JOTRA_GUI_BIN": "/set/new", "WRIT_GUI_BIN": "/set/old"}, "/set/new"),
            ({"JOTRA_GUI_BIN": "", "WRIT_GUI_BIN": ""}, f"{opt}/AppRun"),
        )
        for extra, expected in cases:
            with self.subTest(command=command, env=extra):
                ran = subprocess.run(
                    [str(wrapper)], env={"PATH": os.environ["PATH"], **extra}, capture_output=True, text=True
                )
                self.assertEqual(ran.returncode, 0, ran.stderr)
                self.assertEqual(ran.stdout, f"{expected}\n")


@unittest.skipIf(BASH is None, "bash is not installed")
class PkgbuildFunctionTests(PkgbuildFixture):
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
        self.assertIn(f"-m {self.srcdir}/jotra-{self.version}.AppImage", call)
        self.assertTrue((self.srcdir / f"jotra-{self.version}.AppImage.minisig").read_text().startswith("untrusted comment:"))

    def test_the_command_keeps_a_gui_path_the_person_set(self) -> None:
        self.assert_gui_variable_precedence("jotra")

    @unittest.skipUnless(platform.system() == "Linux", "package() uses GNU install and sed")
    def test_package_marks_the_install_as_pacman_managed(self) -> None:
        self.write_payload()

        result = self.run_function("package")

        self.assertEqual(result.returncode, 0, result.stderr)
        marker = self.pkgdir / "opt/jotra/.package-manager"
        self.assertEqual(marker.read_text(), "pacman\n")
        self.assertEqual(marker.stat().st_mode & 0o777, 0o644)

    @unittest.skipUnless(platform.system() == "Linux", "package() uses GNU install and sed")
    def test_package_installs_the_command_launcher_and_desktop_entry(self) -> None:
        self.write_payload()

        result = self.run_function("package")

        self.assertEqual(result.returncode, 0, result.stderr)
        wrapper = (self.pkgdir / "usr/bin/jotra").read_text()
        self.assertIn(': "${JOTRA_GUI_BIN:=${WRIT_GUI_BIN:-/opt/jotra/AppRun}}"\n', wrapper)
        self.assertIn("export JOTRA_GUI_BIN\n", wrapper)
        self.assertIn('exec "/opt/jotra/usr/bin/jotra" "$@"\n', wrapper)
        self.assertNotIn("JOTRA_COMMAND_NAME", wrapper)
        self.assertIn('exec "/opt/jotra/AppRun" "$@"\n', (self.pkgdir / "usr/bin/jotra-tauri").read_text())
        entry = (self.pkgdir / "usr/share/applications/jotra.desktop").read_text()
        self.assertIn("Exec=/opt/jotra/AppRun %U\n", entry)
        self.assertIn("Icon=jotra\n", entry)
        self.assertIn("StartupWMClass=jotra-tauri\n", entry)
        self.assertNotIn("NoDisplay", entry)
        self.assertTrue((self.pkgdir / "usr/share/icons/hicolor/32x32/apps/jotra.png").is_file())


if __name__ == "__main__":
    unittest.main()
